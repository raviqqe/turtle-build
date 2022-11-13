use crate::{hash_type::HashType, ir::BuildId};
use async_trait::async_trait;
use once_cell::sync::OnceCell;
use std::{error::Error, path::Path, str};

const TIMESTAMP_HASH_TREE_NAME: &str = "timestamp_hash";
const CONTENT_HASH_TREE_NAME: &str = "content_hash";
const OUTPUT_TREE_NAME: &str = "output";
const SOURCE_TREE_NAME: &str = "source";

#[async_trait]
pub trait Database {
    fn initialize(&self, path: &Path) -> Result<(), Box<dyn Error>>;

    fn get_hash(&self, r#type: HashType, id: BuildId) -> Result<Option<u64>, Box<dyn Error>>;
    fn set_hash(&self, r#type: HashType, id: BuildId, hash: u64) -> Result<(), Box<dyn Error>>;

    fn get_outputs(&self) -> Result<Vec<String>, Box<dyn Error>>;
    fn set_output(&self, path: &str) -> Result<(), Box<dyn Error>>;

    fn get_source(&self, output: &str) -> Result<Option<String>, Box<dyn Error>>;
    fn set_source(&self, output: &str, source: &str) -> Result<(), Box<dyn Error>>;

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

    fn hash_database(&self, r#type: HashType) -> Result<sled::Tree, Box<dyn Error>> {
        Ok(self.database()?.open_tree(match r#type {
            HashType::Content => CONTENT_HASH_TREE_NAME,
            HashType::Timestamp => TIMESTAMP_HASH_TREE_NAME,
        })?)
    }

    fn output_database(&self) -> Result<sled::Tree, Box<dyn Error>> {
        Ok(self.database()?.open_tree(OUTPUT_TREE_NAME)?)
    }

    fn source_database(&self) -> Result<sled::Tree, Box<dyn Error>> {
        Ok(self.database()?.open_tree(SOURCE_TREE_NAME)?)
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

    fn get_hash(&self, r#type: HashType, id: BuildId) -> Result<Option<u64>, Box<dyn Error>> {
        Ok(self
            .hash_database(r#type)?
            .get(id.to_bytes())?
            .map(|value| bincode::deserialize(&value))
            .transpose()?)
    }

    fn set_hash(&self, r#type: HashType, id: BuildId, hash: u64) -> Result<(), Box<dyn Error>> {
        self.hash_database(r#type)?
            .insert(id.to_bytes(), bincode::serialize(&hash)?)?;

        Ok(())
    }

    fn get_outputs(&self) -> Result<Vec<String>, Box<dyn Error>> {
        self.output_database()?
            .iter()
            .keys()
            .map(|key| Ok(str::from_utf8(key?.as_ref())?.into()))
            .collect::<Result<_, _>>()
    }

    fn set_output(&self, path: &str) -> Result<(), Box<dyn Error>> {
        self.output_database()?.insert(path, &[])?;

        Ok(())
    }

    fn get_source(&self, output: &str) -> Result<Option<String>, Box<dyn Error>> {
        Ok(if let Some(source) = self.source_database()?.get(output)? {
            Some(str::from_utf8(&source)?.into())
        } else {
            None
        })
    }

    fn set_source(&self, output: &str, source: &str) -> Result<(), Box<dyn Error>> {
        self.source_database()?.insert(output, source)?;

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

    #[tokio::test]
    async fn flush() {
        let database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();
        database.flush().await.unwrap();
    }

    #[test]
    fn timestamp_hash() {
        let database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database
            .set_hash(HashType::Timestamp, BuildId::new(0), 42)
            .unwrap();

        assert_eq!(
            database
                .get_hash(HashType::Timestamp, BuildId::new(0))
                .unwrap(),
            Some(42)
        );
        assert_eq!(
            database
                .get_hash(HashType::Content, BuildId::new(0))
                .unwrap(),
            None,
        );
    }

    #[test]
    fn content_hash() {
        let database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database
            .set_hash(HashType::Content, BuildId::new(0), 42)
            .unwrap();

        assert_eq!(
            database
                .get_hash(HashType::Content, BuildId::new(0))
                .unwrap(),
            Some(42)
        );
        assert_eq!(
            database
                .get_hash(HashType::Timestamp, BuildId::new(0))
                .unwrap(),
            None,
        );
    }

    #[test]
    fn set_output() {
        let database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database.set_output("foo").unwrap();
    }

    #[test]
    fn get_output() {
        let database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database.set_output("foo").unwrap();

        assert_eq!(database.get_outputs().unwrap(), vec!["foo"]);
    }

    #[test]
    fn set_source() {
        let database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database.set_source("foo", "bar").unwrap();
    }

    #[test]
    fn get_source() {
        let database = OsDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database.set_source("foo", "bar").unwrap();

        assert_eq!(database.get_source("foo").unwrap(), Some("bar".into()));
    }
}
