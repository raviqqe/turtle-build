use async_trait::async_trait;
use std::{error::Error, process::Output};
use tokio::{process::Command, sync::Semaphore};

#[async_trait]
pub trait CommandRunner {
    async fn run(&self, command: &str) -> Result<Output, Box<dyn Error>>;
}

#[derive(Debug)]
pub struct OsCommandRunner {
    semaphore: Semaphore,
}

impl OsCommandRunner {
    pub fn new(job_limit: Option<usize>) -> Self {
        Self {
            semaphore: Semaphore::new(job_limit.unwrap_or_else(num_cpus::get)),
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
