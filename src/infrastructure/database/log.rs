mod hash;
mod output;
mod utility;

use self::{hash::HashLog, output::OutputLog};
use crate::{
    build_hash::BuildHash,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
};
use alloc::sync::Arc;
use std::path::Path;
use tokio::{fs::create_dir_all, try_join};

/// A log database.
pub struct LogDatabase {
    hash_log: HashLog,
    output_log: OutputLog,
    fallback: Box<dyn Database + Send + Sync>,
}

impl LogDatabase {
    /// Creates a database.
    pub async fn new(
        directory: &Path,
        fallback: Box<dyn Database + Send + Sync>,
    ) -> Result<Self, DatabaseError> {
        create_dir_all(directory).await?;

        let (hash_log, output_log) = try_join!(HashLog::new(directory), OutputLog::new(directory))?;

        Ok(Self {
            hash_log,
            output_log,
            fallback,
        })
    }
}

impl Database for LogDatabase {
    fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, DatabaseError> {
        Ok(self.hash_log.get(id))
    }

    fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError> {
        self.hash_log.set(id, hash)
    }

    fn get_header_inputs(&self, id: BuildId) -> Result<Vec<Arc<str>>, DatabaseError> {
        self.fallback.get_header_inputs(id)
    }

    fn set_header_inputs(&self, id: BuildId, inputs: &[Arc<str>]) -> Result<(), DatabaseError> {
        self.fallback.set_header_inputs(id, inputs)
    }

    fn get_outputs(&self) -> Result<Vec<String>, DatabaseError> {
        Ok(self.output_log.get())
    }

    fn set_output(&self, path: &str) -> Result<(), DatabaseError> {
        self.output_log.set(path)
    }

    fn get_source(&self, output: &str) -> Result<Option<String>, DatabaseError> {
        self.fallback.get_source(output)
    }

    fn set_source(&self, output: &str, source: &str) -> Result<(), DatabaseError> {
        self.fallback.set_source(output, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::FakeDatabase;
    use pretty_assertions::assert_eq;
    use std::fs::write;
    use tempfile::{TempDir, tempdir};

    async fn open() -> (LogDatabase, TempDir) {
        let directory = tempdir().unwrap();

        (reopen(&directory).await, directory)
    }

    async fn reopen(directory: &TempDir) -> LogDatabase {
        LogDatabase::new(directory.path(), Box::new(FakeDatabase::default()))
            .await
            .unwrap()
    }

    async fn open_with_fallback() -> (LogDatabase, FakeDatabase, TempDir) {
        let directory = tempdir().unwrap();
        let fallback = FakeDatabase::default();

        (
            LogDatabase::new(directory.path(), Box::new(fallback.clone()))
                .await
                .unwrap(),
            fallback,
            directory,
        )
    }

    #[tokio::test]
    async fn new() {
        open().await;
    }

    #[tokio::test]
    async fn new_in_missing_directory() {
        let directory = tempdir().unwrap();

        LogDatabase::new(
            &directory.path().join("foo").join("bar"),
            Box::new(FakeDatabase::default()),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn hash() {
        let (database, _directory) = open().await;

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .unwrap();

        assert_eq!(
            database.get_hash(BuildId::new(0)).unwrap(),
            Some(BuildHash::new(1, 2))
        );
    }

    #[tokio::test]
    async fn get_no_hash() {
        let (database, _directory) = open().await;

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .unwrap();

        assert_eq!(database.get_hash(BuildId::new(1)).unwrap(), None);
    }

    #[tokio::test]
    async fn set_output() {
        let (database, _directory) = open().await;

        database.set_output("foo").unwrap();

        assert_eq!(database.get_outputs().unwrap(), ["foo"]);
    }

    #[tokio::test]
    async fn get_no_output() {
        let (database, _directory) = open().await;

        assert_eq!(database.get_outputs().unwrap(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn get_output_with_source() {
        let (database, _directory) = open().await;

        database.set_output("foo").unwrap();
        database.set_source("foo", "bar").unwrap();

        assert_eq!(database.get_outputs().unwrap(), ["foo"]);
    }

    #[tokio::test]
    async fn reopen_database() {
        let (database, directory) = open().await;

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .unwrap();
        database.set_output("foo").unwrap();

        drop(database);

        let database = reopen(&directory).await;

        assert_eq!(
            database.get_hash(BuildId::new(0)).unwrap(),
            Some(BuildHash::new(1, 2))
        );
        assert_eq!(database.get_outputs().unwrap(), ["foo"]);
    }

    #[tokio::test]
    async fn fail_to_open_invalid_log() {
        let directory = tempdir().unwrap();

        write(directory.path().join("outputs"), [0xff, b'\n']).unwrap();

        assert!(
            LogDatabase::new(directory.path(), Box::new(FakeDatabase::default()))
                .await
                .is_err()
        );
    }

    mod fallback {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn set_no_hash() {
            let (database, fallback, _directory) = open_with_fallback().await;

            database
                .set_hash(BuildId::new(0), BuildHash::new(1, 2))
                .unwrap();

            assert_eq!(fallback.get_hash(BuildId::new(0)).unwrap(), None);
        }

        #[tokio::test]
        async fn get_no_hash() {
            let (database, fallback, _directory) = open_with_fallback().await;

            fallback
                .set_hash(BuildId::new(0), BuildHash::new(1, 2))
                .unwrap();

            assert_eq!(database.get_hash(BuildId::new(0)).unwrap(), None);
        }

        #[tokio::test]
        async fn set_header_inputs() {
            let (database, fallback, _directory) = open_with_fallback().await;

            database
                .set_header_inputs(BuildId::new(0), &["foo".into(), "bar".into()])
                .unwrap();

            assert_eq!(
                fallback.get_header_inputs(BuildId::new(0)).unwrap(),
                vec!["foo".into(), "bar".into()]
            );
        }

        #[tokio::test]
        async fn get_header_inputs() {
            let (database, fallback, _directory) = open_with_fallback().await;

            fallback
                .set_header_inputs(BuildId::new(0), &["foo".into(), "bar".into()])
                .unwrap();

            assert_eq!(
                database.get_header_inputs(BuildId::new(0)).unwrap(),
                vec!["foo".into(), "bar".into()]
            );
        }

        #[tokio::test]
        async fn set_no_output() {
            let (database, fallback, _directory) = open_with_fallback().await;

            database.set_output("foo").unwrap();

            assert_eq!(fallback.get_outputs().unwrap(), Vec::<String>::new());
        }

        #[tokio::test]
        async fn get_no_output() {
            let (database, fallback, _directory) = open_with_fallback().await;

            fallback.set_output("foo").unwrap();

            assert_eq!(database.get_outputs().unwrap(), Vec::<String>::new());
        }

        #[tokio::test]
        async fn set_source() {
            let (database, fallback, _directory) = open_with_fallback().await;

            database.set_source("foo", "bar").unwrap();

            assert_eq!(fallback.get_source("foo").unwrap(), Some("bar".into()));
        }

        #[tokio::test]
        async fn get_source() {
            let (database, fallback, _directory) = open_with_fallback().await;

            fallback.set_source("foo", "bar").unwrap();

            assert_eq!(database.get_source("foo").unwrap(), Some("bar".into()));
        }
    }
}
