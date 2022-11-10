use crate::build_hash::BuildHash;
use crate::ir::BuildId;
use async_trait::async_trait;
use std::error::Error;
use std::path::Path;

#[async_trait]
pub trait Database {
    fn initialize(&mut self, path: &Path) -> Result<(), Box<dyn Error>>;
    fn get(&self, id: BuildId) -> Result<Option<BuildHash>, Box<dyn Error>>;
    fn set(&self, id: BuildId, hash: BuildHash) -> Result<(), Box<dyn Error>>;
    async fn flush(&self) -> Result<(), Box<dyn Error>>;
}

#[derive(Clone, Debug)]
pub struct OsDatabase {
    database: Option<sled::Db>,
}

impl OsDatabase {
    pub fn new() -> Self {
        Self { database: None }
    }

    fn database(&self) -> Result<&sled::Db, Box<dyn Error>> {
        Ok(self
            .database
            .as_ref()
            .ok_or_else(|| "database not initialized")?)
    }
}

#[async_trait]
impl Database for OsDatabase {
    fn initialize(&mut self, path: &Path) -> Result<(), Box<dyn Error>> {
        self.database = Some(sled::open(path)?);

        Ok(())
    }

    fn get(&self, id: BuildId) -> Result<Option<BuildHash>, Box<dyn Error>> {
        Ok(self
            .database()?
            .get(id.to_bytes())?
            .map(|value| bincode::deserialize(&value))
            .transpose()?)
    }

    fn set(&self, id: BuildId, hash: BuildHash) -> Result<(), Box<dyn Error>> {
        self.database()?
            .insert(id.to_bytes(), bincode::serialize(&hash)?)?;

        Ok(())
    }

    async fn flush(&self) -> Result<(), Box<dyn Error>> {
        let database = self.database()?;
        database.flush_async().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn open_database() {
        let mut database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();
    }

    #[test]
    fn set_hash() {
        let mut database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database.set(BuildId::new(0), BuildHash::new(0, 0)).unwrap();
    }

    #[test]
    fn get_hash() {
        let mut database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();
        let hash = BuildHash::new(0, 1);

        database.set(BuildId::new(0), hash).unwrap();
        assert_eq!(database.get(BuildId::new(0)).unwrap(), Some(hash));
    }
}
