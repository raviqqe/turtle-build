#[cfg(test)]
mod fake;
mod os;

#[cfg(test)]
pub use self::fake::FakeConsole;
pub use self::os::OsConsole;
use async_trait::async_trait;
use core::error::Error;

#[async_trait]
pub trait Console {
    async fn write_stdout(&mut self, buffer: &[u8]) -> Result<(), Box<dyn Error>>;
    async fn write_stderr(&mut self, buffer: &[u8]) -> Result<(), Box<dyn Error>>;
    async fn flush(&mut self) -> Result<(), Box<dyn Error>>;
}
