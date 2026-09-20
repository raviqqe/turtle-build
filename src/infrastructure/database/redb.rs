use crate::{
    hash_type::HashType,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
};
use redb::{Durability, Key, ReadOnlyTable, ReadableDatabase, ReadableTable, TableDefinition, Value};
use std::{fs::create_dir_all, path::Path};

const TIMESTAMP_HASHES: TableDefinition<[u8; 8], u64> = TableDefinition::new("timestamp_hashes");
const CONTENT_HASHES: TableDefinition<[u8; 8], u64> = TableDefinition::new("content_hashes");
const HEADER_DEPENDENCIES: TableDefinition<[u8; 8], Vec<&str>> =
    TableDefinition::new("header_dependencies");
const OUTPUTS: TableDefinition<&str, ()> = TableDefinition::new("outputs");
const SOURCES: TableDefinition<&str, &str> = TableDefinition::new("sources");

/// A redb database.
pub struct RedbDatabase {
    database: redb::Database,
}

impl RedbDatabase {
    /// Creates a database.
    pub fn new(path: &Path) -> Result<Self, DatabaseError> {
        Ok(Self {
            database: Self::open(path)?,
        })
    }

    fn open(path: &Path) -> Result<redb::Database, redb::Error> {
        if let Some(directory) = path.parent() {
            create_dir_all(directory)?;
        }

        let database = redb::Database::create(path)?;
        let mut transaction = database.begin_write()?;

        transaction.set_durability(Durability::None)?;
        transaction.open_table(TIMESTAMP_HASHES)?;
        transaction.open_table(CONTENT_HASHES)?;
        transaction.open_table(HEADER_DEPENDENCIES)?;
        transaction.open_table(OUTPUTS)?;
        transaction.open_table(SOURCES)?;
        transaction.commit()?;

        Ok(database)
    }

    fn read<K: Key + 'static, V: Value + 'static, T>(
        &self,
        table: TableDefinition<K, V>,
        read: impl FnOnce(ReadOnlyTable<K, V>) -> Result<T, redb::Error>,
    ) -> Result<T, redb::Error> {
        read(self.database.begin_read()?.open_table(table)?)
    }

    // Writes become durable when the database is closed.
    fn write<K: Key + 'static, V: Value + 'static>(
        &self,
        table: TableDefinition<K, V>,
        key: K::SelfType<'_>,
        value: V::SelfType<'_>,
    ) -> Result<(), redb::Error> {
        let mut transaction = self.database.begin_write()?;

        transaction.set_durability(Durability::None)?;
        transaction.open_table(table)?.insert(key, value)?;
        transaction.commit()?;

        Ok(())
    }
}

const fn hash_table(r#type: HashType) -> TableDefinition<'static, [u8; 8], u64> {
    match r#type {
        HashType::Content => CONTENT_HASHES,
        HashType::Timestamp => TIMESTAMP_HASHES,
    }
}

impl Database for RedbDatabase {
    fn get_hash(&self, r#type: HashType, id: BuildId) -> Result<Option<u64>, DatabaseError> {
        Ok(self.read(hash_table(r#type), |table| {
            Ok(table.get(id.to_bytes())?.map(|hash| hash.value()))
        })?)
    }

    fn set_hash(&self, r#type: HashType, id: BuildId, hash: u64) -> Result<(), DatabaseError> {
        Ok(self.write(hash_table(r#type), id.to_bytes(), hash)?)
    }

    fn get_header_dependencies(&self, id: BuildId) -> Result<Vec<String>, DatabaseError> {
        Ok(self.read(HEADER_DEPENDENCIES, |table| {
            Ok(table
                .get(id.to_bytes())?
                .map(|dependencies| dependencies.value().into_iter().map(From::from).collect())
                .unwrap_or_default())
        })?)
    }

    fn set_header_dependencies(
        &self,
        id: BuildId,
        dependencies: &[String],
    ) -> Result<(), DatabaseError> {
        Ok(self.write(
            HEADER_DEPENDENCIES,
            id.to_bytes(),
            dependencies.iter().map(String::as_str).collect(),
        )?)
    }

    fn get_outputs(&self) -> Result<Vec<String>, DatabaseError> {
        Ok(self.read(OUTPUTS, |table| {
            table
                .iter()?
                .map(|entry| Ok(entry?.0.value().into()))
                .collect()
        })?)
    }

    fn set_output(&self, path: &str) -> Result<(), DatabaseError> {
        Ok(self.write(OUTPUTS, path, ())?)
    }

    fn get_source(&self, output: &str) -> Result<Option<String>, DatabaseError> {
        Ok(self.read(SOURCES, |table| {
            Ok(table.get(output)?.map(|source| source.value().into()))
        })?)
    }

    fn set_source(&self, output: &str, source: &str) -> Result<(), DatabaseError> {
        Ok(self.write(SOURCES, output, source)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{TempDir, tempdir};

    const FILENAME: &str = "database";

    fn open() -> (RedbDatabase, TempDir) {
        let directory = tempdir().unwrap();

        (
            RedbDatabase::new(&directory.path().join(FILENAME)).unwrap(),
            directory,
        )
    }

    #[test]
    fn new() {
        open();
    }

    #[test]
    fn new_in_missing_directory() {
        let directory = tempdir().unwrap();

        RedbDatabase::new(&directory.path().join("foo").join("bar").join(FILENAME)).unwrap();
    }

    #[test]
    fn timestamp_hash() {
        let (database, _directory) = open();

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
        let (database, _directory) = open();

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
    fn update_hash() {
        let (database, _directory) = open();

        database
            .set_hash(HashType::Content, BuildId::new(0), 1)
            .unwrap();
        database
            .set_hash(HashType::Content, BuildId::new(0), 2)
            .unwrap();

        assert_eq!(
            database
                .get_hash(HashType::Content, BuildId::new(0))
                .unwrap(),
            Some(2)
        );
    }

    #[test]
    fn set_output() {
        let (database, _directory) = open();

        database.set_output("foo").unwrap();
    }

    #[test]
    fn get_output() {
        let (database, _directory) = open();

        database.set_output("foo").unwrap();

        assert_eq!(database.get_outputs().unwrap(), vec!["foo"]);
    }

    #[test]
    fn get_no_output() {
        let (database, _directory) = open();

        assert_eq!(database.get_outputs().unwrap(), Vec::<String>::new());
    }

    #[test]
    fn get_output_with_source() {
        let (database, _directory) = open();

        database.set_output("foo").unwrap();
        database.set_source("foo", "bar").unwrap();

        assert_eq!(database.get_outputs().unwrap(), vec!["foo"]);
    }

    #[test]
    fn header_dependencies() {
        let (database, _directory) = open();

        database
            .set_header_dependencies(BuildId::new(0), &["foo".into(), "bar".into()])
            .unwrap();

        assert_eq!(
            database.get_header_dependencies(BuildId::new(0)).unwrap(),
            vec!["foo", "bar"]
        );
    }

    #[test]
    fn get_no_header_dependencies() {
        let (database, _directory) = open();

        assert_eq!(
            database.get_header_dependencies(BuildId::new(0)).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn set_source() {
        let (database, _directory) = open();

        database.set_source("foo", "bar").unwrap();
    }

    #[test]
    fn get_source() {
        let (database, _directory) = open();

        database.set_source("foo", "bar").unwrap();

        assert_eq!(database.get_source("foo").unwrap(), Some("bar".into()));
    }

    #[test]
    fn get_no_source() {
        let (database, _directory) = open();

        assert_eq!(database.get_source("foo").unwrap(), None);
    }

    #[test]
    fn reopen() {
        let (database, directory) = open();

        database
            .set_hash(HashType::Timestamp, BuildId::new(0), 42)
            .unwrap();
        database
            .set_header_dependencies(BuildId::new(0), &["foo".into()])
            .unwrap();
        database.set_output("foo").unwrap();
        database.set_source("foo", "bar").unwrap();

        drop(database);

        let database = RedbDatabase::new(&directory.path().join(FILENAME)).unwrap();

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
