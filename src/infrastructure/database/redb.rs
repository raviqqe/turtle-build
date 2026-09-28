use crate::{
    build_hash::BuildHash,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
    path_pool::PathPool,
};
use alloc::sync::Arc;
use redb::{
    Durability, Key, ReadOnlyTable, ReadableDatabase, ReadableTable, TableDefinition, Value,
};
use std::{fs::create_dir_all, path::Path};

const HASHES: TableDefinition<[u8; 8], (u64, u64)> = TableDefinition::new("hashes");
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
        transaction.open_table(HASHES)?;
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

impl Database for RedbDatabase {
    fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, DatabaseError> {
        Ok(self.read(HASHES, |table| {
            Ok(table
                .get(id.to_bytes())?
                .map(|hash| hash.value())
                .map(|(timestamp, content)| BuildHash::new(timestamp, content)))
        })?)
    }

    fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError> {
        Ok(self.write(HASHES, id.to_bytes(), (hash.timestamp(), hash.content()))?)
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
    fn hash() {
        let (database, _directory) = open();

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .unwrap();

        assert_eq!(
            database.get_hash(BuildId::new(0)).unwrap(),
            Some(BuildHash::new(1, 2))
        );
    }

    #[test]
    fn get_no_hash() {
        let (database, _directory) = open();

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .unwrap();

        assert_eq!(database.get_hash(BuildId::new(1)).unwrap(), None);
    }

    #[test]
    fn update_hash() {
        let (database, _directory) = open();

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .unwrap();
        database
            .set_hash(BuildId::new(0), BuildHash::new(3, 4))
            .unwrap();

        assert_eq!(
            database.get_hash(BuildId::new(0)).unwrap(),
            Some(BuildHash::new(3, 4))
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

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .unwrap();
        database
            .set_header_inputs(BuildId::new(0), &["foo".into()])
            .unwrap();
        database.set_output("foo").unwrap();
        database.set_source("foo", "bar").unwrap();

        drop(database);

        let database =
            RedbDatabase::new(&directory.path().join(FILENAME), Default::default()).unwrap();

        assert_eq!(
            database.get_hash(BuildId::new(0)).unwrap(),
            Some(BuildHash::new(1, 2))
        );
        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).unwrap(),
            vec!["foo".into()]
        );
        assert_eq!(database.get_outputs().unwrap(), vec!["foo"]);
        assert_eq!(database.get_source("foo").unwrap(), Some("bar".into()));
    }
}
