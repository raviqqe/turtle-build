mod error;
#[cfg(test)]
mod fake;
mod os;

#[cfg(test)]
pub use self::fake::FakeCommandRunner;
pub use self::{error::CommandError, os::OsCommandRunner};
use async_trait::async_trait;
use std::process::{ExitStatus, Output};

#[async_trait]
pub trait CommandRunner {
    async fn run(&self, command: &str) -> Result<Output, CommandError>;
    async fn run_with_console(&self, command: &str) -> Result<ExitStatus, CommandError>;
}
