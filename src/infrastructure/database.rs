use crate::build_hash::BuildHash;
use crate::ir::BuildId;
use async_trait::async_trait;
use std::error::Error;
use std::path::Path;

const DATABASE_FILENAME: &str = ".turtle-sled-db";

#[async_trait]
pub trait Database {
    fn get(&self, id: BuildId) -> Result<Option<BuildHash>, Box<dyn Error>>;
    fn set(&self, id: BuildId, hash: BuildHash) -> Result<(), Box<dyn Error>>;
    async fn flush(&self) -> Result<(), Box<dyn Error>>;
}

#[derive(Clone, Debug)]
pub struct OsDatabase {
    database: sled::Db,
}

impl OsDatabase {
    pub fn new(build_directory: &Path) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            database: sled::open(build_directory.join(DATABASE_FILENAME))?,
        })
    }
}

#[async_trait]
impl Database for OsDatabase {
    fn get(&self, id: BuildId) -> Result<Option<BuildHash>, Box<dyn Error>> {
        Ok(self
            .database
            .get(id.to_bytes())?
            .map(|value| bincode::deserialize(&value))
            .transpose()?)
    }

    fn set(&self, id: BuildId, hash: BuildHash) -> Result<(), Box<dyn Error>> {
        self.database
            .insert(id.to_bytes(), bincode::serialize(&hash)?)?;

        Ok(())
    }

    async fn flush(&self) -> Result<(), Box<dyn Error>> {
        self.database.flush_async().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn open_database() {
        OsDatabase::new(tempdir().unwrap().path()).unwrap();
    }

    #[test]
    fn set_hash() {
        let database = OsDatabase::new(tempdir().unwrap().path()).unwrap();

        database.set(BuildId::new(0), BuildHash::new(0, 0)).unwrap();
    }

    #[test]
    fn get_hash() {
        let database = OsDatabase::new(tempdir().unwrap().path()).unwrap();
        let hash = BuildHash::new(0, 1);

        database.set(BuildId::new(0), hash).unwrap();
        assert_eq!(database.get(BuildId::new(0)).unwrap(), Some(hash));
    }
}
