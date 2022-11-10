use async_trait::async_trait;
use std::{error::Error, fmt::Debug};
use tokio::io::{stderr, stdout, AsyncWriteExt};

#[async_trait]
pub trait Console: Debug {
    async fn write_stdout(&self, buffer: &[u8]) -> Result<(), Box<dyn Error>>;
    async fn write_stderr(&self, buffer: &[u8]) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug, Default)]
pub struct OsConsole {}

impl OsConsole {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl Console for OsConsole {
    async fn write_stdout(&self, src: &[u8]) -> Result<(), Box<dyn Error>> {
        stdout().write_all(src).await?;

        Ok(())
    }

    async fn write_stderr(&self, src: &[u8]) -> Result<(), Box<dyn Error>> {
        stderr().write_all(src).await?;

        Ok(())
    }
}
