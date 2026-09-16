#[cfg(test)]
mod fake;
mod os;

#[cfg(test)]
pub use self::fake::FakeCommandRunner;
pub use self::os::OsCommandRunner;
use async_trait::async_trait;
use core::error::Error;
use std::process::Output;

#[async_trait]
pub trait CommandRunner {
    async fn run(&self, command: &str) -> Result<Output, Box<dyn Error>>;
}
