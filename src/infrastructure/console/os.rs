use crate::infrastructure::Console;
use async_trait::async_trait;
use core::error::Error;
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
    async fn write_stdout(&mut self, src: &[u8]) -> Result<(), Box<dyn Error>> {
        self.stdout.write_all(src).await?;

        Ok(())
    }

    async fn write_stderr(&mut self, src: &[u8]) -> Result<(), Box<dyn Error>> {
        self.stderr.write_all(src).await?;

        Ok(())
    }
}
