mod hash;
mod header_input;
mod output;
mod source;
mod utility;

use self::{hash::HashLog, header_input::HeaderInputLog, output::OutputLog, source::SourceLog};
use crate::{
    build_hash::BuildHash,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
    path_pool::PathPool,
};
use alloc::sync::Arc;
use async_trait::async_trait;
use std::path::Path;
use tokio::{fs::create_dir_all, try_join};

const HASH_FILENAME: &str = "hashes";
const HEADER_INPUT_DIRECTORY: &str = "header_inputs";
const OUTPUT_FILENAME: &str = "outputs";
const SOURCE_FILENAME: &str = "sources";

/// A log database.
pub struct LogDatabase {
    hash_log: HashLog,
    header_input_log: HeaderInputLog,
    output_log: OutputLog,
    source_log: SourceLog,
}

impl LogDatabase {
    /// Creates a database.
    pub async fn new(directory: &Path, path_pool: &PathPool) -> Result<Self, DatabaseError> {
        create_dir_all(directory).await?;

        let hash_path = directory.join(HASH_FILENAME);
        let header_input_path = directory.join(HEADER_INPUT_DIRECTORY);
        let output_path = directory.join(OUTPUT_FILENAME);
        let source_path = directory.join(SOURCE_FILENAME);
        let (hash_log, header_input_log, output_log, source_log) = try_join!(
            HashLog::new(&hash_path),
            HeaderInputLog::new(&header_input_path, path_pool),
            OutputLog::new(&output_path),
            SourceLog::new(&source_path)
        )?;

        Ok(Self {
            hash_log,
            header_input_log,
            output_log,
            source_log,
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
        Ok(self.header_input_log.get(id))
    }

    async fn set_header_inputs(
        &self,
        id: BuildId,
        inputs: &[Arc<str>],
    ) -> Result<(), DatabaseError> {
        self.header_input_log.set(id, inputs).await
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
    use pretty_assertions::assert_eq;
    use std::fs::{exists, write};
    use tempfile::{TempDir, tempdir};

    async fn open() -> (LogDatabase, TempDir) {
        let directory = tempdir().unwrap();

        (reopen(&directory).await, directory)
    }

    async fn reopen(directory: &TempDir) -> LogDatabase {
        LogDatabase::new(directory.path(), &Default::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn new() {
        let (_database, directory) = open().await;

        assert!(exists(directory.path().join("hashes")).unwrap());
        assert!(exists(directory.path().join("header_inputs").join("paths")).unwrap());
        assert!(exists(directory.path().join("header_inputs").join("indices")).unwrap());
        assert!(exists(directory.path().join("outputs")).unwrap());
        assert!(exists(directory.path().join("sources")).unwrap());
    }

    #[tokio::test]
    async fn new_in_missing_directory() {
        let directory = tempdir().unwrap();

        LogDatabase::new(
            &directory.path().join("foo").join("bar"),
            &Default::default(),
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
    async fn header_inputs() {
        let (database, _directory) = open().await;

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
    async fn get_no_header_inputs() {
        let (database, _directory) = open().await;

        database
            .set_header_inputs(BuildId::new(0), &["foo".into()])
            .await
            .unwrap();

        assert_eq!(
            database.get_header_inputs(BuildId::new(1)).await.unwrap(),
            Vec::<Arc<str>>::new()
        );
    }

    #[tokio::test]
    async fn intern_header_inputs() {
        let (database, directory) = open().await;

        database
            .set_header_inputs(BuildId::new(0), &["foo".into()])
            .await
            .unwrap();

        drop(database);

        let path_pool = PathPool::new();
        let database = LogDatabase::new(directory.path(), &path_pool)
            .await
            .unwrap();

        assert!(Arc::ptr_eq(
            &database.get_header_inputs(BuildId::new(0)).await.unwrap()[0],
            &path_pool.intern("foo")
        ));
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
        database
            .set_header_inputs(BuildId::new(0), &["foo".into()])
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
        assert_eq!(
            database.get_header_inputs(BuildId::new(0)).await.unwrap(),
            vec!["foo".into()]
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
            LogDatabase::new(directory.path(), &Default::default())
                .await
                .is_err()
        );
    }
}
