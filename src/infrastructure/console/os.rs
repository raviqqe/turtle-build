use crate::infrastructure::{Console, ConsoleError};
use async_trait::async_trait;
use tokio::io::{AsyncWriteExt, Stderr, Stdout, stderr, stdout};

/// A console backed by an operating system.
#[derive(Debug)]
pub struct OsConsole {
    stdout: Stdout,
    stderr: Stderr,
}

impl OsConsole {
    /// Creates a console.
    pub fn new() -> Self {
        Self {
            stdout: stdout(),
            stderr: stderr(),
        }
    }
}

impl Default for OsConsole {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Console for OsConsole {
    async fn write_stdout(&mut self, src: &[u8]) -> Result<(), ConsoleError> {
        self.stdout.write_all(src).await?;

        Ok(())
    }

    async fn write_stderr(&mut self, src: &[u8]) -> Result<(), ConsoleError> {
        self.stderr.write_all(src).await?;

        Ok(())
    }

    async fn flush(&mut self) -> Result<(), ConsoleError> {
        self.stdout.flush().await?;
        self.stderr.flush().await?;

        Ok(())
    }
}
