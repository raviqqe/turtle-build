use async_trait::async_trait;
use std::{error::Error, process::Output};
use tokio::process::Command;

#[async_trait]
pub trait CommandRunner {
    async fn run(&self, command: &str) -> Result<Output, Box<dyn Error>>;
}

#[derive(Debug)]
pub struct OsCommandRunner {}

impl OsCommandRunner {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl CommandRunner for OsCommandRunner {
    async fn run(&self, command: &str) -> Result<Output, Box<dyn Error>> {
        Ok(if cfg!(target_os = "windows") {
            let components = command.split_whitespace().collect::<Vec<_>>();
            Command::new(components[0])
                .args(&components[1..])
                .output()
                .await?
        } else {
            Command::new("sh").arg("-ec").arg(command).output().await?
        })
    }
}
