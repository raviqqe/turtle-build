use crate::infrastructure::{CommandError, CommandRunner};
use async_trait::async_trait;
use std::{
    io,
    process::{ExitStatus, Output, Stdio},
};
use tokio::process::Command;

#[cfg(any(windows, test))]
const BLANK_CHARACTERS: [char; 2] = [' ', '\t'];

/// A command runner backed by an operating system.
#[derive(Debug, Default)]
pub struct OsCommandRunner;

impl OsCommandRunner {
    /// Creates a command runner.
    pub const fn new() -> Self {
        Self
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
        let mut process = Self::create_command(command);

        process
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|error| Self::error(error, &process))
    }

    async fn run_with_console(&self, command: &str) -> Result<ExitStatus, CommandError> {
        let mut process = Self::create_command(command);

        // Inherit standard input, output, and error.
        process
            .status()
            .await
            .map_err(|error| Self::error(error, &process))
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
            let output = OsCommandRunner::new()
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
            let output = OsCommandRunner::new()
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

        #[tokio::test]
        async fn run_failing_command() {
            assert_eq!(
                OsCommandRunner::new()
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
                OsCommandRunner::new()
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
                OsCommandRunner::new()
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
                OsCommandRunner::new()
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
                OsCommandRunner::new()
                    .run_with_console("missing-program foo")
                    .await
                    .unwrap_err()
                    .to_string()
                    .ends_with(": missing-program")
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
