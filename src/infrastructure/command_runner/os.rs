use crate::infrastructure::CommandRunner;
use async_trait::async_trait;
use core::error::Error;
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
    async fn run(&self, command: &str) -> Result<Output, Box<dyn Error>> {
        let permit = self.semaphore.acquire().await?;

        let output = Self::create_command(command)
            .stdin(Stdio::null())
            .output()
            .await?;

        drop(permit);

        Ok(output)
    }

    async fn run_with_console(&self, command: &str) -> Result<ExitStatus, Box<dyn Error>> {
        let permit = self.semaphore.acquire().await?;

        // Inherit standard input, output, and error.
        let status = Self::create_command(command).status().await?;

        drop(permit);

        Ok(status)
    }
}
