mod hash;
mod output;
mod source;
mod utility;

use self::{hash::HashLog, output::OutputLog, source::SourceLog};
use crate::{
    build_hash::BuildHash,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
};
use alloc::sync::Arc;
use async_trait::async_trait;
use std::path::Path;
use tokio::{fs::create_dir_all, try_join};

const HASH_FILENAME: &str = "hashes";
const OUTPUT_FILENAME: &str = "outputs";
const SOURCE_FILENAME: &str = "sources";

/// A log database.
pub struct LogDatabase {
    hash_log: HashLog,
    output_log: OutputLog,
    source_log: SourceLog,
    fallback: Box<dyn Database + Send + Sync>,
}

impl LogDatabase {
    /// Creates a database.
    pub async fn new(
        directory: &Path,
        fallback: Box<dyn Database + Send + Sync>,
    ) -> Result<Self, DatabaseError> {
        create_dir_all(directory).await?;

        let hash_path = directory.join(HASH_FILENAME);
        let output_path = directory.join(OUTPUT_FILENAME);
        let source_path = directory.join(SOURCE_FILENAME);
        let (hash_log, output_log, source_log) = try_join!(
            HashLog::new(&hash_path),
            OutputLog::new(&output_path),
            SourceLog::new(&source_path)
        )?;

        Ok(Self {
            hash_log,
            output_log,
            source_log,
            fallback,
        })
    }
}

#[async_trait]
impl Database for LogDatabase {
    async fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, DatabaseError> {
        Ok(self.hash_log.get(id))
    }

    async fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError> {
        self.hash_log.set(id, hash)
    }

    async fn get_header_inputs(&self, id: BuildId) -> Result<Vec<Arc<str>>, DatabaseError> {
        self.fallback.get_header_inputs(id).await
    }

    async fn set_header_inputs(
        &self,
        id: BuildId,
        inputs: &[Arc<str>],
    ) -> Result<(), DatabaseError> {
        self.fallback.set_header_inputs(id, inputs).await
    }

    async fn get_outputs(&self) -> Result<Vec<String>, DatabaseError> {
        Ok(self.output_log.get())
    }

    async fn set_output(&self, path: &str) -> Result<(), DatabaseError> {
        self.output_log.set(path)
    }

    async fn get_source(&self, output: &str) -> Result<Option<String>, DatabaseError> {
        Ok(self.source_log.get(output))
    }

    async fn set_source(&self, output: &str, source: &str) -> Result<(), DatabaseError> {
        self.source_log.set(output, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::FakeDatabase;
    use pretty_assertions::assert_eq;
    use std::fs::{exists, write};
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
        let (_database, directory) = open().await;

        assert!(exists(directory.path().join("hashes")).unwrap());
        assert!(exists(directory.path().join("outputs")).unwrap());
        assert!(exists(directory.path().join("sources")).unwrap());
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
            .await
            .unwrap();

        assert_eq!(
            database.get_hash(BuildId::new(0)).await.unwrap(),
            Some(BuildHash::new(1, 2))
        );
    }

    #[tokio::test]
    async fn get_no_hash() {
        let (database, _directory) = open().await;

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();

        assert_eq!(database.get_hash(BuildId::new(1)).await.unwrap(), None);
    }

    #[tokio::test]
    async fn set_output() {
        let (database, _directory) = open().await;

        database.set_output("foo").await.unwrap();

        assert_eq!(database.get_outputs().await.unwrap(), ["foo"]);
    }

    #[tokio::test]
    async fn get_no_output() {
        let (database, _directory) = open().await;

        assert_eq!(database.get_outputs().await.unwrap(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn get_output_with_source() {
        let (database, _directory) = open().await;

        database.set_output("foo").await.unwrap();
        database.set_source("foo", "bar").await.unwrap();

        assert_eq!(database.get_outputs().await.unwrap(), ["foo"]);
    }

    #[tokio::test]
    async fn set_source() {
        let (database, _directory) = open().await;

        database.set_source("foo", "bar").await.unwrap();

        assert_eq!(
            database.get_source("foo").await.unwrap(),
            Some("bar".into())
        );
    }

    #[tokio::test]
    async fn get_no_source() {
        let (database, _directory) = open().await;

        database.set_source("foo", "bar").await.unwrap();

        assert_eq!(database.get_source("baz").await.unwrap(), None);
    }

    #[tokio::test]
    async fn reopen_database() {
        let (database, directory) = open().await;

        database
            .set_hash(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();
        database.set_output("foo").await.unwrap();
        database.set_source("foo", "bar").await.unwrap();

        drop(database);

        let database = reopen(&directory).await;

        assert_eq!(
            database.get_hash(BuildId::new(0)).await.unwrap(),
            Some(BuildHash::new(1, 2))
        );
        assert_eq!(database.get_outputs().await.unwrap(), ["foo"]);
        assert_eq!(
            database.get_source("foo").await.unwrap(),
            Some("bar".into())
        );
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
                .await
                .unwrap();

            assert_eq!(fallback.get_hash(BuildId::new(0)).await.unwrap(), None);
        }

        #[tokio::test]
        async fn get_no_hash() {
            let (database, fallback, _directory) = open_with_fallback().await;

            fallback
                .set_hash(BuildId::new(0), BuildHash::new(1, 2))
                .await
                .unwrap();

            assert_eq!(database.get_hash(BuildId::new(0)).await.unwrap(), None);
        }

        #[tokio::test]
        async fn set_header_inputs() {
            let (database, fallback, _directory) = open_with_fallback().await;

            database
                .set_header_inputs(BuildId::new(0), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            assert_eq!(
                fallback.get_header_inputs(BuildId::new(0)).await.unwrap(),
                vec!["foo".into(), "bar".into()]
            );
        }

        #[tokio::test]
        async fn get_header_inputs() {
            let (database, fallback, _directory) = open_with_fallback().await;

            fallback
                .set_header_inputs(BuildId::new(0), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            assert_eq!(
                database.get_header_inputs(BuildId::new(0)).await.unwrap(),
                vec!["foo".into(), "bar".into()]
            );
        }

        #[tokio::test]
        async fn set_no_output() {
            let (database, fallback, _directory) = open_with_fallback().await;

            database.set_output("foo").await.unwrap();

            assert_eq!(fallback.get_outputs().await.unwrap(), Vec::<String>::new());
        }

        #[tokio::test]
        async fn get_no_output() {
            let (database, fallback, _directory) = open_with_fallback().await;

            fallback.set_output("foo").await.unwrap();

            assert_eq!(database.get_outputs().await.unwrap(), Vec::<String>::new());
        }

        #[tokio::test]
        async fn set_no_source() {
            let (database, fallback, _directory) = open_with_fallback().await;

            database.set_source("foo", "bar").await.unwrap();

            assert_eq!(fallback.get_source("foo").await.unwrap(), None);
        }

        #[tokio::test]
        async fn get_no_source() {
            let (database, fallback, _directory) = open_with_fallback().await;

            fallback.set_source("foo", "bar").await.unwrap();

            assert_eq!(database.get_source("foo").await.unwrap(), None);
        }
    }
}
