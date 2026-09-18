use crate::{
    hash_type::HashType,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
};
use async_trait::async_trait;
use core::{
    str,
    sync::atomic::{AtomicBool, Ordering},
};
use fjall::{Keyspace, KeyspaceCreateOptions, PersistMode};
use once_cell::sync::OnceCell;
use std::{path::Path, sync::LazyLock};
use tokio::task::spawn_blocking;

const KEYSPACE_NAME: &str = "build";

const TIMESTAMP_HASH_TAG: u8 = 0;
const CONTENT_HASH_TAG: u8 = 1;
const HEADER_DEPENDENCY_TAG: u8 = 2;
const OUTPUT_TAG: u8 = 3;
const SOURCE_TAG: u8 = 4;

static BINCODE_CONFIG: LazyLock<bincode::config::Configuration> = LazyLock::new(|| {
    bincode::config::Configuration::<
        bincode::config::LittleEndian,
        bincode::config::Varint,
        bincode::config::NoLimit,
    >::default()
});

/// A Fjall database.
#[derive(Default)]
pub struct FjallDatabase {
    database: OnceCell<FjallDatabaseInner>,
    written: AtomicBool,
}

struct FjallDatabaseInner {
    database: fjall::Database,
    keyspace: Keyspace,
}

impl FjallDatabase {
    /// Creates a database.
    pub fn new() -> Self {
        Self::default()
    }

    fn database(&self) -> Result<&FjallDatabaseInner, DatabaseError> {
        self.database
            .get()
            .ok_or_else(|| DatabaseError::new("database not initialized"))
    }

    fn keyspace(&self) -> Result<&Keyspace, DatabaseError> {
        Ok(&self.database()?.keyspace)
    }

    fn insert(&self, key: &[u8], value: &[u8]) -> Result<(), DatabaseError> {
        self.keyspace()?.insert(key, value)?;
        self.written.store(true, Ordering::Relaxed);

        Ok(())
    }
}

const fn hash_tag(r#type: HashType) -> u8 {
    match r#type {
        HashType::Content => CONTENT_HASH_TAG,
        HashType::Timestamp => TIMESTAMP_HASH_TAG,
    }
}

fn key(tag: u8, payload: &[u8]) -> Vec<u8> {
    [[tag].as_slice(), payload].concat()
}

#[async_trait]
impl Database for FjallDatabase {
    fn initialize(&self, path: &Path) -> Result<(), DatabaseError> {
        let database = fjall::Database::builder(path).open()?;

        self.database
            .set(FjallDatabaseInner {
                keyspace: database.keyspace(KEYSPACE_NAME, KeyspaceCreateOptions::default)?,
                database,
            })
            .map_err(|_| DatabaseError::new("database already initialized"))?;

        Ok(())
    }

    fn get_hash(&self, r#type: HashType, id: BuildId) -> Result<Option<u64>, DatabaseError> {
        Ok(self
            .keyspace()?
            .get(key(hash_tag(r#type), &id.to_bytes()))?
            .map(|value| {
                bincode::decode_from_slice(&value, *BINCODE_CONFIG).map(|(value, _)| value)
            })
            .transpose()?)
    }

    fn set_hash(&self, r#type: HashType, id: BuildId, hash: u64) -> Result<(), DatabaseError> {
        self.insert(
            &key(hash_tag(r#type), &id.to_bytes()),
            &bincode::encode_to_vec(hash, *BINCODE_CONFIG)?,
        )
    }

    fn get_header_dependencies(&self, id: BuildId) -> Result<Vec<String>, DatabaseError> {
        Ok(self
            .keyspace()?
            .get(key(HEADER_DEPENDENCY_TAG, &id.to_bytes()))?
            .map(|value| {
                bincode::decode_from_slice(&value, *BINCODE_CONFIG).map(|(value, _)| value)
            })
            .transpose()?
            .unwrap_or_default())
    }

    fn set_header_dependencies(
        &self,
        id: BuildId,
        dependencies: &[String],
    ) -> Result<(), DatabaseError> {
        self.insert(
            &key(HEADER_DEPENDENCY_TAG, &id.to_bytes()),
            &bincode::encode_to_vec(dependencies, *BINCODE_CONFIG)?,
        )
    }

    fn get_outputs(&self) -> Result<Vec<String>, DatabaseError> {
        self.keyspace()?
            .prefix([OUTPUT_TAG])
            .map(|guard| Ok(str::from_utf8(&guard.key()?[1..])?.into()))
            .collect::<Result<_, _>>()
    }

    fn set_output(&self, path: &str) -> Result<(), DatabaseError> {
        self.insert(&key(OUTPUT_TAG, path.as_bytes()), &[])
    }

    fn get_source(&self, output: &str) -> Result<Option<String>, DatabaseError> {
        self.keyspace()?
            .get(key(SOURCE_TAG, output.as_bytes()))?
            .map(|source| Ok::<_, DatabaseError>(str::from_utf8(&source)?.into()))
            .transpose()
    }

    fn set_source(&self, output: &str, source: &str) -> Result<(), DatabaseError> {
        self.insert(&key(SOURCE_TAG, output.as_bytes()), source.as_bytes())
    }

    async fn flush(&self) -> Result<(), DatabaseError> {
        if !self.written.load(Ordering::Relaxed) {
            return Ok(());
        }

        let database = self.database()?.database.clone();

        spawn_blocking(move || database.persist(PersistMode::SyncAll)).await??;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn initialize() {
        let database = FjallDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();
    }

    #[tokio::test]
    async fn flush() {
        let database = FjallDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();
        database.flush().await.unwrap();
    }

    #[tokio::test]
    async fn flush_after_write() {
        let database = FjallDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();
        database.set_output("foo").unwrap();
        database.flush().await.unwrap();
    }

    #[test]
    fn timestamp_hash() {
        let database = FjallDatabase::new();
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
        let database = FjallDatabase::new();
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
        let database = FjallDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database.set_output("foo").unwrap();
    }

    #[test]
    fn get_output() {
        let database = FjallDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database.set_output("foo").unwrap();

        assert_eq!(database.get_outputs().unwrap(), vec!["foo"]);
    }

    #[test]
    fn get_output_with_source() {
        let database = FjallDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database.set_output("foo").unwrap();
        database.set_source("foo", "bar").unwrap();

        assert_eq!(database.get_outputs().unwrap(), vec!["foo"]);
    }

    #[test]
    fn header_dependencies() {
        let database = FjallDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database
            .set_header_dependencies(BuildId::new(0), &["foo".into(), "bar".into()])
            .unwrap();

        assert_eq!(
            database.get_header_dependencies(BuildId::new(0)).unwrap(),
            vec!["foo", "bar"]
        );
    }

    #[test]
    fn set_source() {
        let database = FjallDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database.set_source("foo", "bar").unwrap();
    }

    #[test]
    fn get_source() {
        let database = FjallDatabase::new();
        database.initialize(tempdir().unwrap().path()).unwrap();

        database.set_source("foo", "bar").unwrap();

        assert_eq!(database.get_source("foo").unwrap(), Some("bar".into()));
    }

    #[tokio::test]
    async fn reopen() {
        let directory = tempdir().unwrap();

        let database = FjallDatabase::new();
        database.initialize(directory.path()).unwrap();

        database
            .set_hash(HashType::Timestamp, BuildId::new(0), 42)
            .unwrap();
        database
            .set_header_dependencies(BuildId::new(0), &["foo".into()])
            .unwrap();
        database.set_output("foo").unwrap();
        database.set_source("foo", "bar").unwrap();
        database.flush().await.unwrap();

        drop(database);

        let database = FjallDatabase::new();
        database.initialize(directory.path()).unwrap();

        assert_eq!(
            database
                .get_hash(HashType::Timestamp, BuildId::new(0))
                .unwrap(),
            Some(42)
        );
        assert_eq!(
            database.get_header_dependencies(BuildId::new(0)).unwrap(),
            vec!["foo"]
        );
        assert_eq!(database.get_outputs().unwrap(), vec!["foo"]);
        assert_eq!(database.get_source("foo").unwrap(), Some("bar".into()));
    }
}
