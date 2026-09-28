use crate::{
    build_hash::BuildHash,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
    path_pool::PathPool,
};
use alloc::sync::Arc;
use core::str;
use fjall::Keyspace;
use rkyv::{Archived, access, from_bytes, rancor, to_bytes};

const HASH_TAG: u8 = 0;
const HEADER_DEPENDENCY_TAG: u8 = 1;
const OUTPUT_TAG: u8 = 2;
const SOURCE_TAG: u8 = 3;

/// A Fjall database.
pub struct FjallDatabase {
    keyspace: Keyspace,
    path_pool: Arc<PathPool>,
}

impl FjallDatabase {
    /// Creates a database.
    pub const fn new(keyspace: Keyspace, path_pool: Arc<PathPool>) -> Self {
        Self {
            keyspace,
            path_pool,
        }
    }

    fn insert(&self, key: &[u8], value: &[u8]) -> Result<(), DatabaseError> {
        self.keyspace.insert(key, value)?;

        Ok(())
    }
}

fn key(tag: u8, payload: &[u8]) -> Vec<u8> {
    [[tag].as_slice(), payload].concat()
}

impl Database for FjallDatabase {
    fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, DatabaseError> {
        Ok(self
            .keyspace
            .get(key(HASH_TAG, &id.to_bytes()))?
            .map(|value| {
                from_bytes::<(u64, u64), rancor::Error>(&value)
                    .map(|(timestamp, content)| BuildHash::new(timestamp, content))
            })
            .transpose()?)
    }

    fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError> {
        self.insert(
            &key(HASH_TAG, &id.to_bytes()),
            &to_bytes::<rancor::Error>(&(hash.timestamp(), hash.content()))?,
        )
    }

    fn get_header_inputs(&self, id: BuildId) -> Result<Vec<Arc<str>>, DatabaseError> {
        Ok(self
            .keyspace
            .get(key(HEADER_DEPENDENCY_TAG, &id.to_bytes()))?
            .map(|value| {
                access::<Archived<Vec<Arc<str>>>, rancor::Error>(&value).map(|dependencies| {
                    dependencies
                        .iter()
                        .map(|path| self.path_pool.intern(path))
                        .collect()
                })
            })
            .transpose()?
            .unwrap_or_default())
    }

    fn set_header_inputs(
        &self,
        id: BuildId,
        dependencies: &[Arc<str>],
    ) -> Result<(), DatabaseError> {
        self.insert(
            &key(HEADER_DEPENDENCY_TAG, &id.to_bytes()),
            &to_bytes::<rancor::Error>(&dependencies.to_vec())?,
        )
    }

    fn get_outputs(&self) -> Result<Vec<String>, DatabaseError> {
        self.keyspace
            .prefix([OUTPUT_TAG])
            .map(|guard| Ok(str::from_utf8(&guard.key()?[1..])?.into()))
            .collect::<Result<_, _>>()
    }

    fn set_output(&self, path: &str) -> Result<(), DatabaseError> {
        self.insert(&key(OUTPUT_TAG, path.as_bytes()), &[])
    }

    fn get_source(&self, output: &str) -> Result<Option<String>, DatabaseError> {
        self.keyspace
            .get(key(SOURCE_TAG, output.as_bytes()))?
            .map(|source| Ok::<_, DatabaseError>(str::from_utf8(&source)?.into()))
            .transpose()
    }

    fn set_source(&self, output: &str, source: &str) -> Result<(), DatabaseError> {
        self.insert(&key(SOURCE_TAG, output.as_bytes()), source.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fjall::KeyspaceCreateOptions;
    use std::path::Path;
    use tempfile::tempdir;

    fn open(path: &Path) -> (FjallDatabase, fjall::Database) {
        let database = fjall::Database::builder(path).open().unwrap();

        (
            FjallDatabase::new(
                database
                    .keyspace("build", KeyspaceCreateOptions::default)
                    .unwrap(),
                Default::default(),
            ),
            database,
        )
    }

    #[test]
    fn new() {
        open(tempdir().unwrap().path());
    }

    #[test]
    fn hash() {
        let (database, _fjall) = open(tempdir().unwrap().path());

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
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .unwrap();

        assert_eq!(database.get_hash(BuildId::new(1)).unwrap(), None);
    }

    #[test]
    fn update_hash() {
        let (database, _fjall) = open(tempdir().unwrap().path());

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
    fn fail_to_get_invalid_hash() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .insert(&key(HASH_TAG, &BuildId::new(0).to_bytes()), &[0])
            .unwrap();

        assert!(database.get_hash(BuildId::new(0)).is_err());
    }

    #[test]
    fn set_output() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_output("foo").unwrap();
    }

    #[test]
    fn get_output() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_output("foo").unwrap();

        assert_eq!(database.get_outputs().unwrap(), vec!["foo"]);
    }

    #[test]
    fn get_output_with_source() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_output("foo").unwrap();
        database.set_source("foo", "bar").unwrap();

        assert_eq!(database.get_outputs().unwrap(), vec!["foo"]);
    }

    #[test]
    fn header_dependencies() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .set_header_inputs(BuildId::new(0), &["foo".into(), "bar".into()])
            .unwrap();

        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).unwrap(),
            vec!["foo".into(), "bar".into()]
        );
    }

    #[test]
    fn empty_header_dependencies() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_header_inputs(BuildId::new(0), &[]).unwrap();

        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).unwrap(),
            Vec::<Arc<str>>::new()
        );
    }

    #[test]
    fn long_header_dependency() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .set_header_inputs(BuildId::new(0), &["foo/bar/baz/qux.h".into()])
            .unwrap();

        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).unwrap(),
            vec!["foo/bar/baz/qux.h".into()]
        );
    }

    #[test]
    fn duplicate_header_dependencies() {
        let (database, _fjall) = open(tempdir().unwrap().path());
        let path = Arc::<str>::from("foo");

        database
            .set_header_inputs(BuildId::new(0), &[path.clone(), path])
            .unwrap();

        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).unwrap(),
            vec!["foo".into(), "foo".into()]
        );
    }

    #[test]
    fn intern_header_dependencies() {
        let directory = tempdir().unwrap();
        let path_pool = Arc::new(PathPool::new());
        let database = FjallDatabase::new(
            fjall::Database::builder(directory.path())
                .open()
                .unwrap()
                .keyspace("build", KeyspaceCreateOptions::default)
                .unwrap(),
            path_pool.clone(),
        );

        database
            .set_header_inputs(BuildId::new(0), &["foo".into()])
            .unwrap();

        assert!(Arc::ptr_eq(
            &database.get_header_inputs(BuildId::new(0)).unwrap()[0],
            &path_pool.intern("foo")
        ));
    }

    #[test]
    fn fail_to_get_invalid_header_dependencies() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .insert(
                &key(HEADER_DEPENDENCY_TAG, &BuildId::new(0).to_bytes()),
                &[0xff; 8],
            )
            .unwrap();

        assert!(database.get_header_inputs(BuildId::new(0)).is_err());
    }

    #[test]
    fn set_source() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_source("foo", "bar").unwrap();
    }

    #[test]
    fn get_source() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_source("foo", "bar").unwrap();

        assert_eq!(database.get_source("foo").unwrap(), Some("bar".into()));
    }

    #[test]
    fn reopen() {
        let directory = tempdir().unwrap();

        let (database, fjall) = open(directory.path());

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .unwrap();
        database
            .set_header_inputs(BuildId::new(0), &["foo".into()])
            .unwrap();
        database.set_output("foo").unwrap();
        database.set_source("foo", "bar").unwrap();

        drop(database);
        drop(fjall);

        let (database, _fjall) = open(directory.path());

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
