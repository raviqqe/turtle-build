use crate::{
    hash_type::HashType,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
    path_pool::PathPool,
};
use alloc::sync::Arc;
use redb::{
    Durability, Key, ReadOnlyTable, ReadableDatabase, ReadableTable, TableDefinition, Value,
    WriteTransaction,
};
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
    path_pool: Arc<PathPool>,
}

impl RedbDatabase {
    /// Creates a database.
    pub fn new(path: &Path, path_pool: Arc<PathPool>) -> Result<Self, DatabaseError> {
        Ok(Self {
            database: Self::open(path)?,
            path_pool,
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
    fn transact(
        &self,
        write: impl FnOnce(&WriteTransaction) -> Result<(), redb::Error>,
    ) -> Result<(), redb::Error> {
        let mut transaction = self.database.begin_write()?;

        transaction.set_durability(Durability::None)?;
        write(&transaction)?;
        transaction.commit()?;

        Ok(())
    }

    fn write<K: Key + 'static, V: Value + 'static>(
        &self,
        table: TableDefinition<K, V>,
        key: K::SelfType<'_>,
        value: V::SelfType<'_>,
    ) -> Result<(), redb::Error> {
        self.transact(|transaction| {
            transaction.open_table(table)?.insert(key, value)?;

            Ok(())
        })
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

    fn set_hashes(
        &self,
        id: BuildId,
        timestamp_hash: u64,
        content_hash: u64,
    ) -> Result<(), DatabaseError> {
        Ok(self.transact(|transaction| {
            transaction
                .open_table(TIMESTAMP_HASHES)?
                .insert(id.to_bytes(), timestamp_hash)?;
            transaction
                .open_table(CONTENT_HASHES)?
                .insert(id.to_bytes(), content_hash)?;

            Ok(())
        })?)
    }

    fn get_header_inputs(&self, id: BuildId) -> Result<Vec<Arc<str>>, DatabaseError> {
        Ok(self.read(HEADER_DEPENDENCIES, |table| {
            Ok(table
                .get(id.to_bytes())?
                .map(|dependencies| {
                    dependencies
                        .value()
                        .into_iter()
                        .map(|path| self.path_pool.intern(path))
                        .collect()
                })
                .unwrap_or_default())
        })?)
    }

    fn set_header_inputs(
        &self,
        id: BuildId,
        dependencies: &[Arc<str>],
    ) -> Result<(), DatabaseError> {
        Ok(self.write(
            HEADER_DEPENDENCIES,
            id.to_bytes(),
            dependencies.iter().map(AsRef::as_ref).collect(),
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
            RedbDatabase::new(&directory.path().join(FILENAME), Default::default()).unwrap(),
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

        RedbDatabase::new(
            &directory.path().join("foo").join("bar").join(FILENAME),
            Default::default(),
        )
        .unwrap();
    }

    #[test]
    fn hashes() {
        let (database, _directory) = open();

        database.set_hashes(BuildId::new(0), 1, 2).unwrap();

        assert_eq!(
            database
                .get_hash(HashType::Timestamp, BuildId::new(0))
                .unwrap(),
            Some(1)
        );
        assert_eq!(
            database
                .get_hash(HashType::Content, BuildId::new(0))
                .unwrap(),
            Some(2)
        );
    }

    #[test]
    fn get_no_hash() {
        let (database, _directory) = open();

        database.set_hashes(BuildId::new(0), 1, 2).unwrap();

        assert_eq!(
            database
                .get_hash(HashType::Timestamp, BuildId::new(1))
                .unwrap(),
            None
        );
        assert_eq!(
            database
                .get_hash(HashType::Content, BuildId::new(1))
                .unwrap(),
            None
        );
    }

    #[test]
    fn update_hashes() {
        let (database, _directory) = open();

        database.set_hashes(BuildId::new(0), 1, 2).unwrap();
        database.set_hashes(BuildId::new(0), 3, 4).unwrap();

        assert_eq!(
            database
                .get_hash(HashType::Timestamp, BuildId::new(0))
                .unwrap(),
            Some(3)
        );
        assert_eq!(
            database
                .get_hash(HashType::Content, BuildId::new(0))
                .unwrap(),
            Some(4)
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
            .set_header_inputs(BuildId::new(0), &["foo".into(), "bar".into()])
            .unwrap();

        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).unwrap(),
            vec!["foo".into(), "bar".into()]
        );
    }

    #[test]
    fn get_no_header_dependencies() {
        let (database, _directory) = open();

        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).unwrap(),
            Vec::<Arc<str>>::new()
        );
    }

    #[test]
    fn intern_header_dependencies() {
        let directory = tempdir().unwrap();
        let path_pool = Arc::new(PathPool::new());
        let database =
            RedbDatabase::new(&directory.path().join(FILENAME), path_pool.clone()).unwrap();

        database
            .set_header_inputs(BuildId::new(0), &["foo".into()])
            .unwrap();

        assert!(Arc::ptr_eq(
            &database.get_header_inputs(BuildId::new(0)).unwrap()[0],
            &path_pool.intern("foo")
        ));
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

        database.set_hashes(BuildId::new(0), 1, 2).unwrap();
        database
            .set_header_inputs(BuildId::new(0), &["foo".into()])
            .unwrap();
        database.set_output("foo").unwrap();
        database.set_source("foo", "bar").unwrap();

        drop(database);

        let database =
            RedbDatabase::new(&directory.path().join(FILENAME), Default::default()).unwrap();

        assert_eq!(
            database
                .get_hash(HashType::Timestamp, BuildId::new(0))
                .unwrap(),
            Some(1)
        );
        assert_eq!(
            database
                .get_hash(HashType::Content, BuildId::new(0))
                .unwrap(),
            Some(2)
        );
        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).unwrap(),
            vec!["foo".into()]
        );
        assert_eq!(database.get_outputs().unwrap(), vec!["foo"]);
        assert_eq!(database.get_source("foo").unwrap(), Some("bar".into()));
    }
}
