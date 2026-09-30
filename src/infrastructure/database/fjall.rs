use crate::{
    build_hash::BuildHash,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
    path_pool::PathPool,
};
use alloc::sync::Arc;
use async_trait::async_trait;
use core::str;
use fjall::Keyspace;
use rkyv::{Archived, access, from_bytes, rancor, to_bytes};

const HASH_TAG: u8 = 0;
const HEADER_DEPENDENCY_TAG: u8 = 1;
const OUTPUT_TAG: u8 = 2;

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

#[async_trait]
impl Database for FjallDatabase {
    async fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, DatabaseError> {
        Ok(self
            .keyspace
            .get(key(HASH_TAG, &id.to_bytes()))?
            .map(|value| {
                from_bytes::<(u64, u64), rancor::Error>(&value)
                    .map(|(timestamp, content)| BuildHash::new(timestamp, content))
            })
            .transpose()?)
    }

    async fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError> {
        self.insert(
            &key(HASH_TAG, &id.to_bytes()),
            &to_bytes::<rancor::Error>(&(hash.timestamp(), hash.content()))?,
        )
    }

    async fn get_header_inputs(&self, id: BuildId) -> Result<Vec<Arc<str>>, DatabaseError> {
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

    async fn set_header_inputs(
        &self,
        id: BuildId,
        dependencies: &[Arc<str>],
    ) -> Result<(), DatabaseError> {
        self.insert(
            &key(HEADER_DEPENDENCY_TAG, &id.to_bytes()),
            &to_bytes::<rancor::Error>(&dependencies.to_vec())?,
        )
    }

    async fn get_outputs(&self) -> Result<Vec<String>, DatabaseError> {
        self.keyspace
            .prefix([OUTPUT_TAG])
            .map(|guard| Ok(str::from_utf8(&guard.key()?[1..])?.into()))
            .collect::<Result<_, _>>()
    }

    async fn set_output(&self, path: &str, source: Option<&str>) -> Result<(), DatabaseError> {
        self.insert(
            &key(OUTPUT_TAG, path.as_bytes()),
            &to_bytes::<rancor::Error>(&source.map(String::from))?,
        )
    }

    async fn get_source(&self, output: &str) -> Result<Option<String>, DatabaseError> {
        Ok(self
            .keyspace
            .get(key(OUTPUT_TAG, output.as_bytes()))?
            .map(|value| from_bytes::<Option<String>, rancor::Error>(&value))
            .transpose()?
            .flatten())
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

    #[tokio::test]
    async fn hash() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();

        assert_eq!(
            database.get_hash(BuildId::new(0)).await.unwrap(),
            Some(BuildHash::new(1, 2))
        );
    }

    #[tokio::test]
    async fn get_no_hash() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();

        assert_eq!(database.get_hash(BuildId::new(1)).await.unwrap(), None);
    }

    #[tokio::test]
    async fn update_hash() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();
        database
            .set_hash(BuildId::new(0), BuildHash::new(3, 4))
            .await
            .unwrap();

        assert_eq!(
            database.get_hash(BuildId::new(0)).await.unwrap(),
            Some(BuildHash::new(3, 4))
        );
    }

    #[tokio::test]
    async fn fail_to_get_invalid_hash() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .insert(&key(HASH_TAG, &BuildId::new(0).to_bytes()), &[0])
            .unwrap();

        assert!(database.get_hash(BuildId::new(0)).await.is_err());
    }

    #[tokio::test]
    async fn set_output() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_output("foo", None).await.unwrap();
    }

    #[tokio::test]
    async fn get_output() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_output("foo", None).await.unwrap();

        assert_eq!(database.get_outputs().await.unwrap(), vec!["foo"]);
    }

    #[tokio::test]
    async fn get_output_with_source() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_output("foo", Some("bar")).await.unwrap();

        assert_eq!(database.get_outputs().await.unwrap(), vec!["foo"]);
    }

    #[tokio::test]
    async fn header_dependencies() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .set_header_inputs(BuildId::new(0), &["foo".into(), "bar".into()])
            .await
            .unwrap();

        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).await.unwrap(),
            vec!["foo".into(), "bar".into()]
        );
    }

    #[tokio::test]
    async fn empty_header_dependencies() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .set_header_inputs(BuildId::new(0), &[])
            .await
            .unwrap();

        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).await.unwrap(),
            Vec::<Arc<str>>::new()
        );
    }

    #[tokio::test]
    async fn long_header_dependency() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .set_header_inputs(BuildId::new(0), &["foo/bar/baz/qux.h".into()])
            .await
            .unwrap();

        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).await.unwrap(),
            vec!["foo/bar/baz/qux.h".into()]
        );
    }

    #[tokio::test]
    async fn duplicate_header_dependencies() {
        let (database, _fjall) = open(tempdir().unwrap().path());
        let path = Arc::<str>::from("foo");

        database
            .set_header_inputs(BuildId::new(0), &[path.clone(), path])
            .await
            .unwrap();

        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).await.unwrap(),
            vec!["foo".into(), "foo".into()]
        );
    }

    #[tokio::test]
    async fn intern_header_dependencies() {
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
            .await
            .unwrap();

        assert!(Arc::ptr_eq(
            &database.get_header_inputs(BuildId::new(0)).await.unwrap()[0],
            &path_pool.intern("foo")
        ));
    }

    #[tokio::test]
    async fn fail_to_get_invalid_header_dependencies() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database
            .insert(
                &key(HEADER_DEPENDENCY_TAG, &BuildId::new(0).to_bytes()),
                &[0xff; 8],
            )
            .unwrap();

        assert!(database.get_header_inputs(BuildId::new(0)).await.is_err());
    }

    #[tokio::test]
    async fn get_source() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_output("foo", Some("bar")).await.unwrap();

        assert_eq!(
            database.get_source("foo").await.unwrap(),
            Some("bar".into())
        );
    }

    #[tokio::test]
    async fn get_no_source() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_output("foo", None).await.unwrap();

        assert_eq!(database.get_source("foo").await.unwrap(), None);
    }

    #[tokio::test]
    async fn update_source() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_output("foo", Some("bar")).await.unwrap();
        database.set_output("foo", Some("baz")).await.unwrap();

        assert_eq!(
            database.get_source("foo").await.unwrap(),
            Some("baz".into())
        );
    }

    #[tokio::test]
    async fn remove_source() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.set_output("foo", Some("bar")).await.unwrap();
        database.set_output("foo", None).await.unwrap();

        assert_eq!(database.get_source("foo").await.unwrap(), None);
    }

    #[tokio::test]
    async fn fail_to_get_invalid_source() {
        let (database, _fjall) = open(tempdir().unwrap().path());

        database.insert(&key(OUTPUT_TAG, b"foo"), &[0]).unwrap();

        assert!(database.get_source("foo").await.is_err());
    }

    #[tokio::test]
    async fn reopen() {
        let directory = tempdir().unwrap();

        let (database, fjall) = open(directory.path());

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();
        database
            .set_header_inputs(BuildId::new(0), &["foo".into()])
            .await
            .unwrap();
        database.set_output("foo", Some("bar")).await.unwrap();

        drop(database);
        drop(fjall);

        let (database, _fjall) = open(directory.path());

        assert_eq!(
            database.get_hash(BuildId::new(0)).await.unwrap(),
            Some(BuildHash::new(1, 2))
        );
        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).await.unwrap(),
            vec!["foo".into()]
        );
        assert_eq!(database.get_outputs().await.unwrap(), vec!["foo"]);
        assert_eq!(
            database.get_source("foo").await.unwrap(),
            Some("bar".into())
        );
    }
}
