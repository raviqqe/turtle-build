use crate::{hash_type::HashType, infrastructure::Database, ir::BuildId};
use async_trait::async_trait;
use fjall::{Keyspace, KeyspaceCreateOptions, PersistMode};
use once_cell::sync::OnceCell;
use std::{error::Error, path::Path, str, sync::LazyLock};
use tokio::task::spawn_blocking;

const TIMESTAMP_HASH_KEYSPACE_NAME: &str = "timestamp_hash";
const CONTENT_HASH_KEYSPACE_NAME: &str = "content_hash";
const HEADER_DEPENDENCY_KEYSPACE_NAME: &str = "header_dependency";
const OUTPUT_KEYSPACE_NAME: &str = "output";
const SOURCE_KEYSPACE_NAME: &str = "source";

static BINCODE_CONFIGURATION: LazyLock<bincode::config::Configuration> = LazyLock::new(|| {
    bincode::config::Configuration::<
        bincode::config::LittleEndian,
        bincode::config::Varint,
        bincode::config::NoLimit,
    >::default()
});

#[derive(Default)]
pub struct FjallDatabase {
    database: OnceCell<FjallDatabaseInner>,
}

struct FjallDatabaseInner {
    database: fjall::Database,
    timestamp_hash: Keyspace,
    content_hash: Keyspace,
    header_dependency: Keyspace,
    output: Keyspace,
    source: Keyspace,
}

impl FjallDatabase {
    pub fn new() -> Self {
        Self {
            database: Default::default(),
        }
    }

    fn database(&self) -> Result<&FjallDatabaseInner, Box<dyn Error>> {
        Ok(self.database.get().ok_or("database not initialized")?)
    }

    fn hash_keyspace(&self, r#type: HashType) -> Result<&Keyspace, Box<dyn Error>> {
        let database = self.database()?;

        Ok(match r#type {
            HashType::Content => &database.content_hash,
            HashType::Timestamp => &database.timestamp_hash,
        })
    }
}

#[async_trait]
impl Database for FjallDatabase {
    fn initialize(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        let database = fjall::Database::builder(path).open()?;
        let open = |name: &str| database.keyspace(name, KeyspaceCreateOptions::default);

        self.database
            .set(FjallDatabaseInner {
                timestamp_hash: open(TIMESTAMP_HASH_KEYSPACE_NAME)?,
                content_hash: open(CONTENT_HASH_KEYSPACE_NAME)?,
                header_dependency: open(HEADER_DEPENDENCY_KEYSPACE_NAME)?,
                output: open(OUTPUT_KEYSPACE_NAME)?,
                source: open(SOURCE_KEYSPACE_NAME)?,
                database,
            })
            .map_err(|_| "database already initialized")?;

        Ok(())
    }

    fn get_hash(&self, r#type: HashType, id: BuildId) -> Result<Option<u64>, Box<dyn Error>> {
        Ok(self
            .hash_keyspace(r#type)?
            .get(id.to_bytes())?
            .map(|value| {
                bincode::decode_from_slice(&value, *BINCODE_CONFIGURATION).map(|(value, _)| value)
            })
            .transpose()?)
    }

    fn set_hash(&self, r#type: HashType, id: BuildId, hash: u64) -> Result<(), Box<dyn Error>> {
        self.hash_keyspace(r#type)?.insert(
            id.to_bytes(),
            bincode::encode_to_vec(hash, *BINCODE_CONFIGURATION)?,
        )?;

        Ok(())
    }

    fn get_header_dependencies(&self, id: BuildId) -> Result<Vec<String>, Box<dyn Error>> {
        Ok(self
            .database()?
            .header_dependency
            .get(id.to_bytes())?
            .map(|value| {
                bincode::decode_from_slice(&value, *BINCODE_CONFIGURATION).map(|(value, _)| value)
            })
            .transpose()?
            .unwrap_or_default())
    }

    fn set_header_dependencies(
        &self,
        id: BuildId,
        dependencies: &[String],
    ) -> Result<(), Box<dyn Error>> {
        self.database()?.header_dependency.insert(
            id.to_bytes(),
            bincode::encode_to_vec(dependencies, *BINCODE_CONFIGURATION)?,
        )?;

        Ok(())
    }

    fn get_outputs(&self) -> Result<Vec<String>, Box<dyn Error>> {
        self.database()?
            .output
            .iter()
            .map(|guard| Ok(str::from_utf8(&guard.key()?)?.into()))
            .collect::<Result<_, _>>()
    }

    fn set_output(&self, path: &str) -> Result<(), Box<dyn Error>> {
        self.database()?.output.insert(path, [])?;

        Ok(())
    }

    fn get_source(&self, output: &str) -> Result<Option<String>, Box<dyn Error>> {
        self.database()?
            .source
            .get(output)?
            .map(|source| Ok::<_, Box<dyn Error>>(str::from_utf8(&source)?.into()))
            .transpose()
    }

    fn set_source(&self, output: &str, source: &str) -> Result<(), Box<dyn Error>> {
        self.database()?.source.insert(output, source)?;

        Ok(())
    }

    async fn flush(&self) -> Result<(), Box<dyn Error>> {
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
