use crate::infrastructure::{CommandError, CommandRunner};
use async_trait::async_trait;
use std::process::{ExitStatus, Output, Stdio};
use tokio::{process::Command, sync::Semaphore};

/// A command runner backed by an operating system.
#[derive(Debug)]
pub struct OsCommandRunner {
    semaphore: Semaphore,
}

impl OsCommandRunner {
    /// Creates a command runner.
    pub fn new(job_limit: usize) -> Self {
        Self {
            semaphore: Semaphore::new(job_limit),
        }
    }

    fn create_command(command: &str) -> Command {
        if cfg!(target_os = "windows") {
            let components = command.split_whitespace().collect::<Vec<_>>();
            let mut process = Command::new(components[0]);

            process.args(&components[1..]);
            process
        } else {
            let mut process = Command::new("sh");

            process.arg("-ec").arg(command);
            process
        }
    }
}

#[async_trait]
impl CommandRunner for OsCommandRunner {
    async fn run(&self, command: &str) -> Result<Output, CommandError> {
        let _permit = self.semaphore.acquire().await?;
        let mut process = Self::create_command(command);

        // Detach a command from a terminal so that it cannot read input meant for
        // a command in the console pool.
        #[cfg(unix)]
        process.process_group(0);

        Ok(process.stdin(Stdio::null()).output().await?)
    }

    async fn run_with_console(&self, command: &str) -> Result<ExitStatus, CommandError> {
        let _permit = self.semaphore.acquire().await?;

        // Inherit standard input, output, and error.
        Ok(Self::create_command(command).status().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
