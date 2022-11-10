use crate::{build_hash::BuildHash, ir::BuildId};
use async_trait::async_trait;
use std::{error::Error, path::Path};

#[async_trait]
pub trait CommandRunner {
    async fn run(&self, command: &str, arguments: &[&str]) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug)]
pub struct OsCommandRunner {}

impl OsCommandRunner {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl CommandRunner for OsDatabase {
    fn initialize(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        self.command_runner
            .set(sled::open(path)?)
            .map_err(|_| "command_runner already initialized")?;

        Ok(())
    }

    fn get(&self, id: BuildId) -> Result<Option<BuildHash>, Box<dyn Error>> {
        Ok(self
            .command_runner()?
            .get(id.to_bytes())?
            .map(|value| bincode::deserialize(&value))
            .transpose()?)
    }

    fn set(&self, id: BuildId, hash: BuildHash) -> Result<(), Box<dyn Error>> {
        self.command_runner()?
            .insert(id.to_bytes(), bincode::serialize(&hash)?)?;

        Ok(())
    }

    async fn flush(&self) -> Result<(), Box<dyn Error>> {
        let command_runner = self.database()?;
        command_runner.flush_async().await?;

        Ok(())
    }
}
