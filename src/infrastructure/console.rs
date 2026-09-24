mod buffer;
mod error;
#[cfg(test)]
mod fake;
mod os;

#[cfg(test)]
pub use self::fake::FakeConsole;
pub use self::{buffer::ConsoleBuffer, error::ConsoleError, os::OsConsole};
use async_trait::async_trait;

/// A console.
#[async_trait]
pub trait Console {
    /// Writes bytes to standard output.
    async fn write_stdout(&mut self, buffer: &[u8]) -> Result<(), ConsoleError>;
    /// Writes bytes to standard error.
    async fn write_stderr(&mut self, buffer: &[u8]) -> Result<(), ConsoleError>;
    /// Flushes standard output and error.
    async fn flush(&mut self) -> Result<(), ConsoleError>;
}
