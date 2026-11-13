use crate::infrastructure::{CommandError, CommandRunner};
use async_trait::async_trait;
#[cfg(unix)]
use rustix::process::{Pid, Signal, kill_process_group};
use std::{
    io,
    process::{ExitStatus, Output, Stdio},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    select,
    sync::{Semaphore, watch},
    try_join,
};

#[cfg(any(windows, test))]
const BLANK_CHARACTERS: [char; 2] = [' ', '\t'];

/// A command runner backed by an operating system.
#[derive(Debug)]
pub struct OsCommandRunner {
    semaphore: Semaphore,
    killed: watch::Sender<()>,
}

impl OsCommandRunner {
    /// Creates a command runner.
    pub fn new(job_limit: usize) -> Self {
        Self {
            semaphore: Semaphore::new(job_limit),
            killed: watch::Sender::new(()),
        }
    }

    fn create_command(command: &str) -> Command {
        cfg_select! {
            windows => {
                let (program, arguments) = split_program(command);
                let mut process = Command::new(program);

                process.raw_arg(arguments);
                process
            }
            _ => {
                let mut process = Command::new("sh");

                process.arg("-ec").arg(command);
                process
            }
        }
    }

    fn terminate(child: &mut Child) {
        cfg_select! {
            unix => {
                if let Some(id) = child
                    .id()
                    .and_then(|id| Pid::from_raw(id.try_into().ok()?))
                {
                    kill_process_group(id, Signal::TERM).ok();
                }
            }
            _ => {
                child.start_kill().ok();
            }
        }
    }

    async fn read(stream: &mut Option<impl AsyncRead + Unpin>) -> io::Result<Vec<u8>> {
        let mut buffer = vec![];

        if let Some(stream) = stream {
            stream.read_to_end(&mut buffer).await?;
        }

        Ok(buffer)
    }

    fn error(error: io::Error, process: &Command) -> CommandError {
        CommandError::new(format!(
            "{}: {}",
            error,
            process.as_std().get_program().display()
        ))
    }
}

#[async_trait]
impl CommandRunner for OsCommandRunner {
    async fn run(&self, command: &str) -> Result<Output, CommandError> {
        let mut killed = self.killed.subscribe();
        let _permit = self.semaphore.acquire().await?;
        let mut process = Self::create_command(command);

        // Group a command with its descendants to terminate them all at once.
        #[cfg(unix)]
        process.process_group(0);

        let mut child = process
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| Self::error(error, &process))?;
        let (mut stdout, mut stderr) = (child.stdout.take(), child.stderr.take());
        let (status, stdout, stderr) = try_join!(
            async {
                select! {
                    status = child.wait() => status.map(Some),
                    _ = killed.changed() => {
                        Self::terminate(&mut child);
                        child.wait().await?;

                        Ok(None)
                    }
                }
            },
            Self::read(&mut stdout),
            Self::read(&mut stderr),
        )
        .map_err(|error| Self::error(error, &process))?;

        Ok(Output {
            status: status.ok_or_else(|| CommandError::new("command killed"))?,
            stdout,
            stderr,
        })
    }

    async fn run_with_console(&self, command: &str) -> Result<ExitStatus, CommandError> {
        // A command with a console is never killed, but a kill waits for it.
        let _killed = self.killed.subscribe();
        let _permit = self.semaphore.acquire().await?;
        let mut process = Self::create_command(command);

        // Inherit standard input, output, and error.
        process
            .status()
            .await
            .map_err(|error| Self::error(error, &process))
    }

    async fn kill(&self) {
        // Close a semaphore first so that every command started is notified.
        self.semaphore.close();
        self.killed.send_replace(());
        self.killed.closed().await;
    }
}

#[cfg(any(windows, test))]
fn split_program(command: &str) -> (String, &str) {
    let command = command.trim_start_matches(BLANK_CHARACTERS);
    let (program, arguments) = command.split_at(
        command
            .char_indices()
            .scan(false, |quoted, (index, character)| {
                *quoted ^= character == '"';

                Some((index, !*quoted && BLANK_CHARACTERS.contains(&character)))
            })
            .find(|&(_, end)| end)
            .map_or(command.len(), |(index, _)| index),
    );

    (
        program.replace('"', ""),
        arguments.trim_start_matches(BLANK_CHARACTERS),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    mod run {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn run_command() {
            let output = OsCommandRunner::new(1)
                .run(cfg_select! {
                    windows => "cmd /c echo foo",
                    _ => "echo foo",
                })
                .await
                .unwrap();

            assert!(output.status.success());
            assert_eq!(String::from_utf8(output.stdout).unwrap().trim_end(), "foo");
        }

        #[tokio::test]
        async fn run_command_with_quoted_argument() {
            let output = OsCommandRunner::new(1)
                .run(cfg_select! {
                    windows => "cmd /c \"echo foo  bar\"",
                    _ => "echo 'foo  bar'",
                })
                .await
                .unwrap();

            assert!(output.status.success());
            assert_eq!(
                String::from_utf8(output.stdout).unwrap().trim_end(),
                "foo  bar"
            );
        }

        // cspell: ignore pgid
        #[cfg(unix)]
        #[tokio::test]
        async fn run_command_in_own_process_group() {
            let output = OsCommandRunner::new(1)
                .run("ps -o pid= -o pgid= -p $$")
                .await
                .unwrap();
            let output = String::from_utf8(output.stdout).unwrap();
            let ids = output.split_whitespace().collect::<Vec<_>>();

            assert_eq!(ids.len(), 2);
            assert_eq!(ids[0], ids[1]);
        }

        #[tokio::test]
        async fn run_failing_command() {
            assert_eq!(
                OsCommandRunner::new(1)
                    .run(cfg_select! {
                        windows => "cmd /c exit 42",
                        _ => "exit 42",
                    })
                    .await
                    .unwrap()
                    .status
                    .code(),
                Some(42)
            );
        }

        #[cfg(windows)]
        #[tokio::test]
        async fn fail_to_run_missing_program() {
            assert!(
                OsCommandRunner::new(1)
                    .run("missing-program foo")
                    .await
                    .unwrap_err()
                    .to_string()
                    .ends_with(": missing-program")
            );
        }
    }

    mod run_with_console {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn run_command() {
            assert!(
                OsCommandRunner::new(1)
                    .run_with_console(cfg_select! {
                        windows => "cmd /c exit 0",
                        _ => "exit 0",
                    })
                    .await
                    .unwrap()
                    .success()
            );
        }

        #[tokio::test]
        async fn run_failing_command() {
            assert_eq!(
                OsCommandRunner::new(1)
                    .run_with_console(cfg_select! {
                        windows => "cmd /c exit 42",
                        _ => "exit 42",
                    })
                    .await
                    .unwrap()
                    .code(),
                Some(42)
            );
        }

        #[cfg(windows)]
        #[tokio::test]
        async fn fail_to_run_missing_program() {
            assert!(
                OsCommandRunner::new(1)
                    .run_with_console("missing-program foo")
                    .await
                    .unwrap_err()
                    .to_string()
                    .ends_with(": missing-program")
            );
        }
    }

    mod kill {
        use super::*;
        use core::pin::pin;
        use futures::poll;
        use pretty_assertions::assert_eq;
        use tokio::join;

        #[tokio::test]
        async fn kill_no_command() {
            OsCommandRunner::new(1).kill().await;
        }

        #[tokio::test]
        async fn kill_command() {
            let runner = OsCommandRunner::new(1);
            let mut future = pin!(runner.run(cfg_select! {
                windows => "ping -n 60 127.0.0.1",
                _ => "sleep 60",
            }));

            assert!(poll!(&mut future).is_pending());

            let (result, ()) = join!(future, runner.kill());

            assert_eq!(result, Err(CommandError::new("command killed")));
        }

        #[cfg(unix)]
        #[tokio::test]
        async fn kill_descendant_of_command() {
            use core::time::Duration;
            use tempfile::tempdir;
            use tokio::{
                fs::try_exists,
                time::{sleep, timeout},
            };

            let directory = tempdir().unwrap();
            let file = directory.path().join("foo");
            let command = format!("sleep 60 & touch {} && wait", file.display());
            let runner = OsCommandRunner::new(1);

            // A command finishes only after its descendant closes standard output and error.
            let (result, ()) = timeout(Duration::from_secs(10), async {
                join!(runner.run(&command), async {
                    while !try_exists(&file).await.unwrap() {
                        sleep(Duration::from_millis(10)).await;
                    }

                    runner.kill().await;
                })
            })
            .await
            .unwrap();

            assert_eq!(result, Err(CommandError::new("command killed")));
        }

        #[tokio::test]
        async fn wait_for_console_command() {
            let runner = OsCommandRunner::new(1);
            let mut command = pin!(runner.run_with_console(cfg_select! {
                windows => "ping -n 2 127.0.0.1",
                _ => "sleep 0.1",
            }));
            let mut kill = pin!(runner.kill());

            assert!(poll!(&mut command).is_pending());
            assert!(poll!(&mut kill).is_pending());
            assert!(command.await.unwrap().success());

            kill.await;
        }

        #[tokio::test]
        async fn run_no_command_after_kill() {
            let runner = OsCommandRunner::new(1);

            runner.kill().await;

            assert!(
                runner
                    .run(cfg_select! {
                        windows => "cmd /c exit 0",
                        _ => "exit 0",
                    })
                    .await
                    .is_err()
            );
            assert!(
                runner
                    .run_with_console(cfg_select! {
                        windows => "cmd /c exit 0",
                        _ => "exit 0",
                    })
                    .await
                    .is_err()
            );
        }
    }

    mod split_program {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn split_no_argument() {
            assert_eq!(split_program("foo"), ("foo".into(), ""));
        }

        #[test]
        fn split_arguments() {
            assert_eq!(split_program("foo bar baz"), ("foo".into(), "bar baz"));
            assert_eq!(split_program("foo\tbar"), ("foo".into(), "bar"));
            assert_eq!(split_program("foo  bar  baz "), ("foo".into(), "bar  baz "));
        }

        #[test]
        fn split_quoted_arguments() {
            assert_eq!(
                split_program("foo \"bar baz\" \\\"qux"),
                ("foo".into(), "\"bar baz\" \\\"qux")
            );
        }

        #[test]
        fn split_leading_blanks() {
            assert_eq!(split_program(" \tfoo bar"), ("foo".into(), "bar"));
        }

        #[test]
        fn split_quoted_program() {
            assert_eq!(
                split_program("\"C:\\Program Files\\foo.exe\" bar"),
                ("C:\\Program Files\\foo.exe".into(), "bar")
            );
            assert_eq!(split_program("\"foo bar\""), ("foo bar".into(), ""));
        }

        #[test]
        fn split_partially_quoted_program() {
            assert_eq!(
                split_program("\"C:\\Program Files\"\\foo.exe bar"),
                ("C:\\Program Files\\foo.exe".into(), "bar")
            );
        }

        #[test]
        fn split_program_with_unclosed_quote() {
            assert_eq!(split_program("\"foo bar"), ("foo bar".into(), ""));
        }

        #[test]
        fn split_empty_command() {
            assert_eq!(split_program(""), ("".into(), ""));
            assert_eq!(split_program(" "), ("".into(), ""));
        }
    }
}
