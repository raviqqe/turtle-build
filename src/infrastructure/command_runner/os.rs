use crate::infrastructure::CommandRunner;
use async_trait::async_trait;
use core::error::Error;
use std::process::Output;
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
}

#[async_trait]
impl CommandRunner for OsCommandRunner {
    async fn run(&self, command: &str) -> Result<Output, Box<dyn Error>> {
        let permit = self.semaphore.acquire().await?;

        let output = if cfg!(target_os = "windows") {
            let components = command.split_whitespace().collect::<Vec<_>>();
            Command::new(components[0])
                .args(&components[1..])
                .output()
                .await?
        } else {
            Command::new("sh").arg("-ec").arg(command).output().await?
        };

        drop(permit);

        Ok(output)
    }
}
