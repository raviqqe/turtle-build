use crate::{
    build_hash::BuildHash,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
};
use alloc::sync::Arc;
use scc::{Guard, HashIndex, hash_index::Entry};
use std::{
    fs::File,
    io::{self, ErrorKind, Write},
    path::Path,
};
use tokio::fs::{OpenOptions, create_dir_all, read, rename, write};

const COMPACTION_RATIO: usize = 3;
const HASH_FILENAME: &str = "hashes";
const TEMPORARY_EXTENSION: &str = "tmp";

type Record = [[u8; size_of::<u64>()]; 3];

/// A log database.
pub struct LogDatabase {
    file: File,
    hashes: HashIndex<BuildId, BuildHash>,
    fallback: Box<dyn Database + Send + Sync>,
}

impl LogDatabase {
    /// Creates a database.
    pub async fn new(
        directory: &Path,
        fallback: Box<dyn Database + Send + Sync>,
    ) -> Result<Self, DatabaseError> {
        create_dir_all(directory).await?;

        let (file, hashes) = Self::open(&directory.join(HASH_FILENAME)).await?;

        Ok(Self {
            file,
            hashes,
            fallback,
        })
    }

    async fn open(path: &Path) -> Result<(File, HashIndex<BuildId, BuildHash>), io::Error> {
        let bytes = read(path).await.or_else(|error| {
            if error.kind() == ErrorKind::NotFound {
                Ok(vec![])
            } else {
                Err(error)
            }
        })?;
        let records = bytes.as_chunks().0.as_chunks().0;
        let hashes = HashIndex::with_capacity(records.len());

        // Newer records override older ones.
        for &record in records.iter().rev() {
            let (id, hash) = deserialize(record);

            hashes.insert_sync(id, hash).ok();
        }

        // A log ends with an incomplete record when its last write was interrupted.
        if !bytes.len().is_multiple_of(size_of::<Record>())
            || bytes.len() > COMPACTION_RATIO * size_of::<Record>() * hashes.len()
        {
            let temporary_path = path.with_added_extension(TEMPORARY_EXTENSION);
            // Do not inline this to avoid holding a guard across an await point.
            let bytes = hashes
                .iter(&Guard::new())
                .flat_map(|(&id, &hash)| serialize(id, hash))
                .flatten()
                .collect::<Vec<_>>();

            write(&temporary_path, bytes).await?;
            rename(temporary_path, path).await?;
        }

        Ok((
            OpenOptions::new()
                .append(true)
                .create(true)
                .open(path)
                .await?
                .into_std()
                .await,
            hashes,
        ))
    }
}

const fn serialize(id: BuildId, hash: BuildHash) -> Record {
    [
        id.to_bytes(),
        hash.timestamp().to_le_bytes(),
        hash.content().to_le_bytes(),
    ]
}

const fn deserialize([id, timestamp, content]: Record) -> (BuildId, BuildHash) {
    (
        BuildId::from_bytes(id),
        BuildHash::new(u64::from_le_bytes(timestamp), u64::from_le_bytes(content)),
    )
}

impl Database for LogDatabase {
    fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, DatabaseError> {
        Ok(self.hashes.peek_with(&id, |_, hash| *hash))
    }

    fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError> {
        (&self.file).write_all(serialize(id, hash).as_flattened())?;

        match self.hashes.entry_sync(id) {
            Entry::Occupied(mut entry) => entry.update(hash),
            Entry::Vacant(entry) => {
                entry.insert_entry(hash);
            }
        }

        Ok(())
    }

    fn get_header_inputs(&self, id: BuildId) -> Result<Vec<Arc<str>>, DatabaseError> {
        self.fallback.get_header_inputs(id)
    }

    fn set_header_inputs(&self, id: BuildId, inputs: &[Arc<str>]) -> Result<(), DatabaseError> {
        self.fallback.set_header_inputs(id, inputs)
    }

    fn get_outputs(&self) -> Result<Vec<String>, DatabaseError> {
        self.fallback.get_outputs()
    }

    fn set_output(&self, path: &str) -> Result<(), DatabaseError> {
        self.fallback.set_output(path)
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
    use std::{
        fs::{exists, read, write},
        thread::scope,
    };
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

    fn read_log(directory: &TempDir) -> Vec<u8> {
        read(directory.path().join(HASH_FILENAME)).unwrap()
    }

    fn write_log(directory: &TempDir, records: &[Record]) {
        write(
            directory.path().join(HASH_FILENAME),
            records.as_flattened().as_flattened(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn new() {
        let (_database, directory) = open().await;

        assert!(exists(directory.path().join("hashes")).unwrap());
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
    async fn update_hash() {
        let (database, _directory) = open().await;

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

    #[tokio::test]
    async fn set_hashes_concurrently() {
        const THREAD_COUNT: u64 = 8;
        const BUILD_COUNT: u64 = 256;

        let (database, directory) = open().await;

        scope(|scope| {
            for thread in 0..THREAD_COUNT {
                let database = &database;

                scope.spawn(move || {
                    for build in 0..BUILD_COUNT {
                        let id = BUILD_COUNT * thread + build;

                        database
                            .set_hash(BuildId::new(id), BuildHash::new(id + 1, id + 2))
                            .unwrap();
                    }
                });
            }
        });

        drop(database);

        let database = reopen(&directory).await;

        for id in 0..THREAD_COUNT * BUILD_COUNT {
            assert_eq!(
                database.get_hash(BuildId::new(id)).unwrap(),
                Some(BuildHash::new(id + 1, id + 2))
            );
        }
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
        async fn set_output() {
            let (database, fallback, _directory) = open_with_fallback().await;

            database.set_output("foo").unwrap();

            assert_eq!(fallback.get_outputs().unwrap(), vec!["foo"]);
        }

        #[tokio::test]
        async fn get_outputs() {
            let (database, fallback, _directory) = open_with_fallback().await;

            fallback.set_output("foo").unwrap();

            assert_eq!(database.get_outputs().unwrap(), vec!["foo"]);
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

    mod log {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn write_record() {
            let (database, directory) = open().await;

            database
                .set_hash(BuildId::new(1), BuildHash::new(2, 3))
                .unwrap();

            assert_eq!(
                read_log(&directory),
                [
                    [1, 0, 0, 0, 0, 0, 0, 0],
                    [2, 0, 0, 0, 0, 0, 0, 0],
                    [3, 0, 0, 0, 0, 0, 0, 0]
                ]
                .as_flattened()
            );
        }

        #[tokio::test]
        async fn write_record_in_little_endian() {
            let (database, directory) = open().await;

            database
                .set_hash(
                    BuildId::new(0x0102_0304_0506_0708),
                    BuildHash::new(0x1112_1314_1516_1718, 0x2122_2324_2526_2728),
                )
                .unwrap();

            assert_eq!(
                read_log(&directory),
                [
                    [0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01],
                    [0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11],
                    [0x28, 0x27, 0x26, 0x25, 0x24, 0x23, 0x22, 0x21]
                ]
                .as_flattened()
            );
        }

        #[tokio::test]
        async fn append_records() {
            let (database, directory) = open().await;

            database
                .set_hash(BuildId::new(0), BuildHash::new(1, 2))
                .unwrap();
            database
                .set_hash(BuildId::new(3), BuildHash::new(4, 5))
                .unwrap();
            database
                .set_hash(BuildId::new(0), BuildHash::new(6, 7))
                .unwrap();

            assert_eq!(
                read_log(&directory),
                [
                    serialize(BuildId::new(0), BuildHash::new(1, 2)),
                    serialize(BuildId::new(3), BuildHash::new(4, 5)),
                    serialize(BuildId::new(0), BuildHash::new(6, 7))
                ]
                .as_flattened()
                .as_flattened()
            );
        }

        #[tokio::test]
        async fn reopen_log() {
            let (database, directory) = open().await;

            database
                .set_hash(BuildId::new(0), BuildHash::new(1, 2))
                .unwrap();
            database
                .set_hash(BuildId::new(3), BuildHash::new(4, 5))
                .unwrap();

            drop(database);

            let database = reopen(&directory).await;

            assert_eq!(
                database.get_hash(BuildId::new(0)).unwrap(),
                Some(BuildHash::new(1, 2))
            );
            assert_eq!(
                database.get_hash(BuildId::new(3)).unwrap(),
                Some(BuildHash::new(4, 5))
            );
        }

        #[tokio::test]
        async fn reopen_log_with_updated_hash() {
            let (database, directory) = open().await;

            database
                .set_hash(BuildId::new(0), BuildHash::new(1, 2))
                .unwrap();
            database
                .set_hash(BuildId::new(0), BuildHash::new(3, 4))
                .unwrap();

            drop(database);

            assert_eq!(
                reopen(&directory).await.get_hash(BuildId::new(0)).unwrap(),
                Some(BuildHash::new(3, 4))
            );
        }

        #[tokio::test]
        async fn append_records_after_reopen() {
            let (database, directory) = open().await;

            database
                .set_hash(BuildId::new(0), BuildHash::new(1, 2))
                .unwrap();

            drop(database);

            reopen(&directory)
                .await
                .set_hash(BuildId::new(3), BuildHash::new(4, 5))
                .unwrap();

            assert_eq!(
                read_log(&directory),
                [
                    serialize(BuildId::new(0), BuildHash::new(1, 2)),
                    serialize(BuildId::new(3), BuildHash::new(4, 5))
                ]
                .as_flattened()
                .as_flattened()
            );
        }
    }

    mod compaction {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn compact() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                &[
                    serialize(BuildId::new(0), BuildHash::new(1, 2)),
                    serialize(BuildId::new(0), BuildHash::new(3, 4)),
                    serialize(BuildId::new(0), BuildHash::new(5, 6)),
                    serialize(BuildId::new(0), BuildHash::new(7, 8)),
                ],
            );

            let database = reopen(&directory).await;

            assert_eq!(
                database.get_hash(BuildId::new(0)).unwrap(),
                Some(BuildHash::new(7, 8))
            );
            assert_eq!(
                read_log(&directory),
                serialize(BuildId::new(0), BuildHash::new(7, 8)).as_flattened()
            );
        }

        #[tokio::test]
        async fn compact_many_builds() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                &[
                    serialize(BuildId::new(0), BuildHash::new(1, 2)),
                    serialize(BuildId::new(10), BuildHash::new(11, 12)),
                    serialize(BuildId::new(0), BuildHash::new(3, 4)),
                    serialize(BuildId::new(10), BuildHash::new(13, 14)),
                    serialize(BuildId::new(0), BuildHash::new(5, 6)),
                    serialize(BuildId::new(10), BuildHash::new(15, 16)),
                    serialize(BuildId::new(0), BuildHash::new(7, 8)),
                ],
            );

            let database = reopen(&directory).await;

            assert_eq!(
                database.get_hash(BuildId::new(0)).unwrap(),
                Some(BuildHash::new(7, 8))
            );
            assert_eq!(
                database.get_hash(BuildId::new(10)).unwrap(),
                Some(BuildHash::new(15, 16))
            );

            let mut records = read_log(&directory)
                .as_chunks()
                .0
                .as_chunks()
                .0
                .iter()
                .map(|&record| deserialize(record))
                .collect::<Vec<_>>();

            records.sort_by_key(|(id, _)| id.to_bytes());

            assert_eq!(
                records,
                [
                    (BuildId::new(0), BuildHash::new(7, 8)),
                    (BuildId::new(10), BuildHash::new(15, 16))
                ]
            );
        }

        #[tokio::test]
        async fn compact_after_appending_records() {
            let (database, directory) = open().await;

            for hash in 0..4 {
                database
                    .set_hash(BuildId::new(0), BuildHash::new(hash, hash))
                    .unwrap();
            }

            drop(database);

            assert_eq!(read_log(&directory).len(), 4 * size_of::<Record>());

            let database = reopen(&directory).await;

            assert_eq!(
                database.get_hash(BuildId::new(0)).unwrap(),
                Some(BuildHash::new(3, 3))
            );
            assert_eq!(
                read_log(&directory),
                serialize(BuildId::new(0), BuildHash::new(3, 3)).as_flattened()
            );
        }

        #[tokio::test]
        async fn append_record_after_compaction() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                &[
                    serialize(BuildId::new(0), BuildHash::new(1, 2)),
                    serialize(BuildId::new(0), BuildHash::new(3, 4)),
                    serialize(BuildId::new(0), BuildHash::new(5, 6)),
                    serialize(BuildId::new(0), BuildHash::new(7, 8)),
                ],
            );

            reopen(&directory)
                .await
                .set_hash(BuildId::new(9), BuildHash::new(10, 11))
                .unwrap();

            assert_eq!(
                read_log(&directory),
                [
                    serialize(BuildId::new(0), BuildHash::new(7, 8)),
                    serialize(BuildId::new(9), BuildHash::new(10, 11))
                ]
                .as_flattened()
                .as_flattened()
            );
        }

        #[tokio::test]
        async fn remove_temporary_file() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                &[serialize(BuildId::new(0), BuildHash::new(1, 2)); 4],
            );

            reopen(&directory).await;

            assert_eq!(read_log(&directory).len(), size_of::<Record>());
            assert!(
                !exists(
                    directory
                        .path()
                        .join(HASH_FILENAME)
                        .with_added_extension(TEMPORARY_EXTENSION)
                )
                .unwrap()
            );
        }

        #[tokio::test]
        async fn overwrite_temporary_file() {
            let directory = tempdir().unwrap();

            write(
                directory
                    .path()
                    .join(HASH_FILENAME)
                    .with_added_extension(TEMPORARY_EXTENSION),
                [0; 42],
            )
            .unwrap();
            write_log(
                &directory,
                &[serialize(BuildId::new(0), BuildHash::new(1, 2)); 4],
            );

            reopen(&directory).await;

            assert_eq!(
                read_log(&directory),
                serialize(BuildId::new(0), BuildHash::new(1, 2)).as_flattened()
            );
        }

        #[tokio::test]
        async fn keep_log_of_optimal_size() {
            let directory = tempdir().unwrap();
            let records = [
                serialize(BuildId::new(0), BuildHash::new(1, 2)),
                serialize(BuildId::new(3), BuildHash::new(4, 5)),
            ];

            write_log(&directory, &records);

            reopen(&directory).await;

            assert_eq!(read_log(&directory), records.as_flattened().as_flattened());
        }

        #[tokio::test]
        async fn keep_log_of_compaction_ratio() {
            let directory = tempdir().unwrap();
            let records = [
                serialize(BuildId::new(0), BuildHash::new(1, 2)),
                serialize(BuildId::new(0), BuildHash::new(3, 4)),
                serialize(BuildId::new(0), BuildHash::new(5, 6)),
            ];

            write_log(&directory, &records);

            let database = reopen(&directory).await;

            assert_eq!(
                database.get_hash(BuildId::new(0)).unwrap(),
                Some(BuildHash::new(5, 6))
            );
            assert_eq!(read_log(&directory), records.as_flattened().as_flattened());
        }

        #[tokio::test]
        async fn keep_empty_log() {
            let directory = tempdir().unwrap();

            write_log(&directory, &[]);

            reopen(&directory).await;

            assert_eq!(read_log(&directory), [0u8; 0]);
        }
    }

    mod incomplete_record {
        use super::*;
        use pretty_assertions::assert_eq;

        fn write_incomplete_log(directory: &TempDir, records: &[Record], size: usize) {
            write(
                directory.path().join(HASH_FILENAME),
                [records.as_flattened().as_flattened(), &vec![0xff; size]].concat(),
            )
            .unwrap();
        }

        #[tokio::test]
        async fn remove_incomplete_record() {
            for size in 1..size_of::<Record>() {
                let directory = tempdir().unwrap();

                write_incomplete_log(
                    &directory,
                    &[serialize(BuildId::new(0), BuildHash::new(1, 2))],
                    size,
                );

                let database = reopen(&directory).await;

                assert_eq!(
                    database.get_hash(BuildId::new(0)).unwrap(),
                    Some(BuildHash::new(1, 2))
                );
                assert_eq!(
                    read_log(&directory),
                    serialize(BuildId::new(0), BuildHash::new(1, 2)).as_flattened()
                );
            }
        }

        #[tokio::test]
        async fn remove_only_incomplete_record() {
            let directory = tempdir().unwrap();

            write_incomplete_log(&directory, &[], 1);

            reopen(&directory).await;

            assert_eq!(read_log(&directory), [0u8; 0]);
        }

        #[tokio::test]
        async fn append_record_after_incomplete_record() {
            let directory = tempdir().unwrap();

            write_incomplete_log(
                &directory,
                &[serialize(BuildId::new(0), BuildHash::new(1, 2))],
                1,
            );

            reopen(&directory)
                .await
                .set_hash(BuildId::new(3), BuildHash::new(4, 5))
                .unwrap();

            let database = reopen(&directory).await;

            assert_eq!(
                database.get_hash(BuildId::new(0)).unwrap(),
                Some(BuildHash::new(1, 2))
            );
            assert_eq!(
                database.get_hash(BuildId::new(3)).unwrap(),
                Some(BuildHash::new(4, 5))
            );
        }
    }
}
