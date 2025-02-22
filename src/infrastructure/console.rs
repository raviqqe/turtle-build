use async_trait::async_trait;
use std::{error::Error, fmt::Debug};
use tokio::io::{AsyncWriteExt, Stderr, Stdout, stderr, stdout};

#[async_trait]
pub trait Console {
    async fn write_stdout(&mut self, buffer: &[u8]) -> Result<(), Box<dyn Error>>;
    async fn write_stderr(&mut self, buffer: &[u8]) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug)]
pub struct OsConsole {
    stdout: Stdout,
    stderr: Stderr,
}

impl OsConsole {
    pub fn new() -> Self {
        Self {
            stdout: stdout(),
            stderr: stderr(),
        }
    }
}

#[async_trait]
impl Console for OsConsole {
    async fn write_stdout(&mut self, src: &[u8]) -> Result<(), Box<dyn Error>> {
        self.stdout.write_all(src).await?;

        Ok(())
    }

    async fn write_stderr(&mut self, src: &[u8]) -> Result<(), Box<dyn Error>> {
        self.stderr.write_all(src).await?;

        Ok(())
    }
}
