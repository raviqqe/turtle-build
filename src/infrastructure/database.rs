use crate::{build_hash::BuildHash, ir::BuildId};
use async_trait::async_trait;
use once_cell::sync::OnceCell;
use std::{error::Error, path::Path};

const HASH_TREE_NAME: &str = "hash";
const OUTPUT_TREE_NAME: &str = "output";

#[async_trait]
pub trait Database {
    fn initialize(&self, path: &Path) -> Result<(), Box<dyn Error>>;
    fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, Box<dyn Error>>;
    fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), Box<dyn Error>>;
    fn get_outputs(&self) -> Result<Vec<String>, Box<dyn Error>>;
    fn set_output(&self, path: &str) -> Result<(), Box<dyn Error>>;
    async fn flush(&self) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug)]
pub struct OsDatabase {
    database: OnceCell<sled::Db>,
}

impl OsDatabase {
    pub fn new() -> Self {
        Self {
            database: Default::default(),
        }
    }

    fn database(&self) -> Result<&sled::Db, Box<dyn Error>> {
        Ok(self.database.get().ok_or("database not initialized")?)
    }

    fn hash_database(&self) -> Result<sled::Tree, Box<dyn Error>> {
        Ok(self.database()?.open_tree(HASH_TREE_NAME)?)
    }

    fn output_database(&self) -> Result<sled::Tree, Box<dyn Error>> {
        Ok(self.database()?.open_tree(OUTPUT_TREE_NAME)?)
    }
}

#[async_trait]
impl Database for OsDatabase {
    fn initialize(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        self.database
            .set(sled::open(path)?)
            .map_err(|_| "database already initialized")?;

        Ok(())
    }

    fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, Box<dyn Error>> {
        Ok(self
            .hash_database()?
            .get(id.to_bytes())?
            .map(|value| bincode::deserialize(&value))
            .transpose()?)
    }

    fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), Box<dyn Error>> {
        self.hash_database()?
            .insert(id.to_bytes(), bincode::serialize(&hash)?)?;

        Ok(())
    }

    fn get_outputs(&self) -> Result<Vec<String>, Box<dyn Error>> {
        self.output_database()?
            .iter()
            .keys()
            .map(|key| Ok(String::from_utf8_lossy(key?.as_ref()).into()))
            .collect::<Result<_, _>>()
    }

    fn set_output(&self, path: &str) -> Result<(), Box<dyn Error>> {
        self.output_database()?.insert(path.as_bytes(), &[])?;

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
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn initialize() {
        let database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();
    }

    #[test]
    fn set_hash() {
        let database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database
            .set_hash(BuildId::new(0), BuildHash::new(0, 0))
            .unwrap();
    }

    #[test]
    fn get_hash() {
        let database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();
        let hash = BuildHash::new(0, 1);

        database.set_hash(BuildId::new(0), hash).unwrap();
        assert_eq!(database.get_hash(BuildId::new(0)).unwrap(), Some(hash));
    }

    #[tokio::test]
    async fn flush() {
        let database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();
        database.flush().await.unwrap();
    }
}
