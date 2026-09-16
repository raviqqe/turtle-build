mod error;
#[cfg(test)]
mod fake;
mod os;

#[cfg(test)]
pub use self::fake::FakeConsole;
pub use self::{error::ConsoleError, os::OsConsole};
use async_trait::async_trait;

#[async_trait]
pub trait Console {
    async fn write_stdout(&mut self, buffer: &[u8]) -> Result<(), ConsoleError>;
    async fn write_stderr(&mut self, buffer: &[u8]) -> Result<(), ConsoleError>;
    async fn flush(&mut self) -> Result<(), ConsoleError>;
}
