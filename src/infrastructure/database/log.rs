use crate::{
    build_hash::BuildHash,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
};
use alloc::sync::Arc;
use core::str;
use scc::{Guard, HashIndex, hash_index::Entry};
use std::{
    fs::File,
    io::{self, ErrorKind, Write},
    path::Path,
};
use tokio::{
    fs::{OpenOptions, create_dir_all, read, rename, write},
    try_join,
};

const COMPACTION_RATIO: usize = 3;
const HASH_FILENAME: &str = "hashes";
const OUTPUT_FILENAME: &str = "outputs";
const TEMPORARY_EXTENSION: &str = "tmp";
const LINE_TERMINATOR: u8 = b'\n';

type Record = [[u8; size_of::<u64>()]; 3];

/// A log database.
pub struct LogDatabase {
    hash_file: File,
    hashes: HashIndex<BuildId, BuildHash>,
    output_file: File,
    outputs: HashIndex<String, ()>,
    fallback: Box<dyn Database + Send + Sync>,
}

impl LogDatabase {
    /// Creates a database.
    pub async fn new(
        directory: &Path,
        fallback: Box<dyn Database + Send + Sync>,
    ) -> Result<Self, DatabaseError> {
        create_dir_all(directory).await?;

        let ((hash_file, hashes), (output_file, outputs)) =
            try_join!(Self::open_hashes(directory), Self::open_outputs(directory))?;

        Ok(Self {
            hash_file,
            hashes,
            output_file,
            outputs,
            fallback,
        })
    }

    async fn open_hashes(
        directory: &Path,
    ) -> Result<(File, HashIndex<BuildId, BuildHash>), DatabaseError> {
        let path = directory.join(HASH_FILENAME);
        let bytes = read_file(&path).await?;
        let records = bytes.as_chunks().0.as_chunks().0;
        let hashes = HashIndex::with_capacity(records.len());

        for &record in records.iter().rev() {
            let (id, hash) = deserialize(record);

            hashes.insert_sync(id, hash).ok();
        }

        if !bytes.len().is_multiple_of(size_of::<Record>())
            || bytes.len() > COMPACTION_RATIO * size_of::<Record>() * hashes.len()
        {
            // Do not inline this to avoid holding a guard across an await point.
            let bytes = hashes
                .iter(&Guard::new())
                .flat_map(|(&id, &hash)| serialize(id, hash))
                .flatten()
                .collect::<Vec<_>>();

            compact_file(&path, bytes).await?;
        }

        Ok((open_file(&path).await?, hashes))
    }

    async fn open_outputs(
        directory: &Path,
    ) -> Result<(File, HashIndex<String, ()>), DatabaseError> {
        let path = directory.join(OUTPUT_FILENAME);
        let bytes = read_file(&path).await?;
        let lines = bytes
            .split_inclusive(|&byte| byte == LINE_TERMINATOR)
            .filter_map(|line| line.strip_suffix(&[LINE_TERMINATOR]))
            .collect::<Vec<_>>();
        let outputs = HashIndex::<String, _>::with_capacity(lines.len());

        for line in lines {
            outputs.insert_sync(str::from_utf8(line)?.into(), ()).ok();
        }

        if bytes.last().is_some_and(|&byte| byte != LINE_TERMINATOR)
            || bytes.len()
                > COMPACTION_RATIO
                    * outputs
                        .iter(&Guard::new())
                        .map(|(path, _)| path.len() + size_of_val(&LINE_TERMINATOR))
                        .sum::<usize>()
        {
            // Do not inline this to avoid holding a guard across an await point.
            let bytes = outputs
                .iter(&Guard::new())
                .flat_map(|(path, _)| serialize_output(path))
                .collect::<Vec<_>>();

            compact_file(&path, bytes).await?;
        }

        Ok((open_file(&path).await?, outputs))
    }
}

async fn read_file(path: &Path) -> Result<Vec<u8>, io::Error> {
    read(path).await.or_else(|error| {
        if error.kind() == ErrorKind::NotFound {
            Ok(vec![])
        } else {
            Err(error)
        }
    })
}

async fn compact_file(path: &Path, bytes: Vec<u8>) -> Result<(), io::Error> {
    let temporary_path = path.with_added_extension(TEMPORARY_EXTENSION);

    write(&temporary_path, bytes).await?;
    rename(temporary_path, path).await
}

async fn open_file(path: &Path) -> Result<File, io::Error> {
    Ok(OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .await?
        .into_std()
        .await)
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

fn serialize_output(path: &str) -> Vec<u8> {
    [path.as_bytes(), &[LINE_TERMINATOR]].concat()
}

impl Database for LogDatabase {
    fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, DatabaseError> {
        Ok(self.hashes.peek_with(&id, |_, hash| *hash))
    }

    fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError> {
        (&self.hash_file).write_all(serialize(id, hash).as_flattened())?;

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
        Ok(self
            .outputs
            .iter(&Guard::new())
            .map(|(path, _)| path.clone())
            .collect())
    }

    fn set_output(&self, path: &str) -> Result<(), DatabaseError> {
        if self.outputs.insert_sync(path.into(), ()).is_ok() {
            (&self.output_file).write_all(&serialize_output(path))?;
        }

        Ok(())
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

    mod output {
        use super::*;
        use pretty_assertions::assert_eq;

        fn get_outputs(database: &LogDatabase) -> Vec<String> {
            let mut outputs = database.get_outputs().unwrap();

            outputs.sort();

            outputs
        }

        fn read_log(directory: &TempDir) -> Vec<u8> {
            read(directory.path().join(OUTPUT_FILENAME)).unwrap()
        }

        fn read_lines(directory: &TempDir) -> Vec<String> {
            let mut lines = String::from_utf8(read_log(directory))
                .unwrap()
                .split_inclusive('\n')
                .map(From::from)
                .collect::<Vec<_>>();

            lines.sort();

            lines
        }

        fn write_log(directory: &TempDir, log: impl AsRef<[u8]>) {
            write(directory.path().join(OUTPUT_FILENAME), log).unwrap();
        }

        #[tokio::test]
        async fn new() {
            let (_database, directory) = open().await;

            assert!(exists(directory.path().join("outputs")).unwrap());
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
        async fn set_outputs() {
            let (database, _directory) = open().await;

            database.set_output("foo").unwrap();
            database.set_output("bar").unwrap();

            assert_eq!(get_outputs(&database), ["bar", "foo"]);
        }

        #[tokio::test]
        async fn set_output_twice() {
            let (database, _directory) = open().await;

            database.set_output("foo").unwrap();
            database.set_output("foo").unwrap();

            assert_eq!(database.get_outputs().unwrap(), ["foo"]);
        }

        #[tokio::test]
        async fn set_output_in_directory() {
            let (database, _directory) = open().await;

            database.set_output("foo/bar baz.o").unwrap();

            assert_eq!(database.get_outputs().unwrap(), ["foo/bar baz.o"]);
        }

        #[tokio::test]
        async fn set_output_with_source() {
            let (database, _directory) = open().await;

            database.set_output("foo").unwrap();
            database.set_source("foo", "bar").unwrap();

            assert_eq!(database.get_outputs().unwrap(), ["foo"]);
        }

        #[tokio::test]
        async fn set_outputs_concurrently() {
            const THREAD_COUNT: usize = 8;
            const OUTPUT_COUNT: usize = 256;

            let (database, directory) = open().await;
            let outputs = (0..THREAD_COUNT * OUTPUT_COUNT)
                .map(|index| format!("{index:04}"))
                .collect::<Vec<_>>();

            scope(|scope| {
                for outputs in outputs.chunks(OUTPUT_COUNT) {
                    let database = &database;

                    scope.spawn(move || {
                        for output in outputs {
                            database.set_output(output).unwrap();
                        }
                    });
                }
            });

            assert_eq!(get_outputs(&database), outputs);

            drop(database);

            assert_eq!(get_outputs(&reopen(&directory).await), outputs);
        }

        mod log {
            use super::*;
            use pretty_assertions::assert_eq;

            #[tokio::test]
            async fn write_line() {
                let (database, directory) = open().await;

                database.set_output("foo").unwrap();

                assert_eq!(read_log(&directory), b"foo\n");
            }

            #[tokio::test]
            async fn write_line_in_utf8() {
                let (database, directory) = open().await;

                database.set_output("😄").unwrap();

                assert_eq!(read_log(&directory), [0xf0, 0x9f, 0x98, 0x84, b'\n']);
            }

            #[tokio::test]
            async fn append_lines() {
                let (database, directory) = open().await;

                database.set_output("foo").unwrap();
                database.set_output("bar").unwrap();

                assert_eq!(read_log(&directory), b"foo\nbar\n");
            }

            #[tokio::test]
            async fn append_no_line_of_same_output() {
                let (database, directory) = open().await;

                database.set_output("foo").unwrap();
                database.set_output("bar").unwrap();
                database.set_output("foo").unwrap();

                assert_eq!(read_log(&directory), b"foo\nbar\n");
            }

            #[tokio::test]
            async fn reopen_log() {
                let (database, directory) = open().await;

                database.set_output("foo").unwrap();
                database.set_output("bar").unwrap();

                drop(database);

                assert_eq!(get_outputs(&reopen(&directory).await), ["bar", "foo"]);
            }

            #[tokio::test]
            async fn reopen_log_in_utf8() {
                let (database, directory) = open().await;

                database.set_output("😄").unwrap();

                drop(database);

                assert_eq!(reopen(&directory).await.get_outputs().unwrap(), ["😄"]);
            }

            #[tokio::test]
            async fn reopen_log_with_empty_line() {
                let directory = tempdir().unwrap();

                write_log(&directory, "foo\n\nbar\n");

                assert_eq!(get_outputs(&reopen(&directory).await), ["", "bar", "foo"]);
            }

            #[tokio::test]
            async fn append_lines_after_reopen() {
                let (database, directory) = open().await;

                database.set_output("foo").unwrap();

                drop(database);

                reopen(&directory).await.set_output("bar").unwrap();

                assert_eq!(read_log(&directory), b"foo\nbar\n");
            }

            #[tokio::test]
            async fn append_no_line_of_same_output_after_reopen() {
                let (database, directory) = open().await;

                database.set_output("foo").unwrap();

                drop(database);

                reopen(&directory).await.set_output("foo").unwrap();

                assert_eq!(read_log(&directory), b"foo\n");
            }

            #[tokio::test]
            async fn fail_to_open_log_in_invalid_utf8() {
                let directory = tempdir().unwrap();

                write_log(&directory, [b'f', 0xff, b'\n']);

                assert!(
                    LogDatabase::new(directory.path(), Box::new(FakeDatabase::default()))
                        .await
                        .is_err()
                );
            }
        }

        mod compaction {
            use super::*;
            use pretty_assertions::assert_eq;

            #[tokio::test]
            async fn compact() {
                let directory = tempdir().unwrap();

                write_log(&directory, "foo\nfoo\nfoo\nfoo\n");

                let database = reopen(&directory).await;

                assert_eq!(database.get_outputs().unwrap(), ["foo"]);
                assert_eq!(read_log(&directory), b"foo\n");
            }

            #[tokio::test]
            async fn compact_many_outputs() {
                let directory = tempdir().unwrap();

                write_log(&directory, "foo\nbar\nfoo\nbar\nfoo\nbar\nfoo\n");

                let database = reopen(&directory).await;

                assert_eq!(get_outputs(&database), ["bar", "foo"]);
                assert_eq!(read_lines(&directory), ["bar\n", "foo\n"]);
            }

            #[tokio::test]
            async fn compact_outputs_of_different_lengths() {
                let directory = tempdir().unwrap();

                write_log(&directory, "foo\nfoo\nfoo\nfoo\nfoo\nfoo\nbarbaz\n");

                let database = reopen(&directory).await;

                assert_eq!(get_outputs(&database), ["barbaz", "foo"]);
                assert_eq!(read_lines(&directory), ["barbaz\n", "foo\n"]);
            }

            #[tokio::test]
            async fn append_line_after_compaction() {
                let directory = tempdir().unwrap();

                write_log(&directory, "foo\nfoo\nfoo\nfoo\n");

                reopen(&directory).await.set_output("bar").unwrap();

                assert_eq!(read_log(&directory), b"foo\nbar\n");
            }

            #[tokio::test]
            async fn remove_temporary_file() {
                let directory = tempdir().unwrap();

                write_log(&directory, "foo\nfoo\nfoo\nfoo\n");

                reopen(&directory).await;

                assert_eq!(read_log(&directory), b"foo\n");
                assert!(
                    !exists(
                        directory
                            .path()
                            .join(OUTPUT_FILENAME)
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
                        .join(OUTPUT_FILENAME)
                        .with_added_extension(TEMPORARY_EXTENSION),
                    [0; 42],
                )
                .unwrap();
                write_log(&directory, "foo\nfoo\nfoo\nfoo\n");

                reopen(&directory).await;

                assert_eq!(read_log(&directory), b"foo\n");
            }

            #[tokio::test]
            async fn keep_log_of_optimal_size() {
                let directory = tempdir().unwrap();

                write_log(&directory, "foo\nbar\n");

                reopen(&directory).await;

                assert_eq!(read_log(&directory), b"foo\nbar\n");
            }

            #[tokio::test]
            async fn keep_log_of_compaction_ratio() {
                let directory = tempdir().unwrap();

                write_log(&directory, "foo\nfoo\nfoo\n");

                let database = reopen(&directory).await;

                assert_eq!(database.get_outputs().unwrap(), ["foo"]);
                assert_eq!(read_log(&directory), b"foo\nfoo\nfoo\n");
            }

            #[tokio::test]
            async fn keep_log_of_compaction_ratio_with_outputs_of_different_lengths() {
                let directory = tempdir().unwrap();

                write_log(&directory, "foo\nfoo\nfoo\nfoo\nfoo\nbarbaz\n");

                let database = reopen(&directory).await;

                assert_eq!(get_outputs(&database), ["barbaz", "foo"]);
                assert_eq!(read_log(&directory), b"foo\nfoo\nfoo\nfoo\nfoo\nbarbaz\n");
            }

            #[tokio::test]
            async fn keep_empty_log() {
                let directory = tempdir().unwrap();

                write_log(&directory, "");

                reopen(&directory).await;

                assert_eq!(read_log(&directory), b"");
            }
        }

        mod incomplete_line {
            use super::*;
            use pretty_assertions::assert_eq;

            #[tokio::test]
            async fn remove_incomplete_line() {
                let directory = tempdir().unwrap();

                write_log(&directory, "foo\nba");

                let database = reopen(&directory).await;

                assert_eq!(database.get_outputs().unwrap(), ["foo"]);
                assert_eq!(read_log(&directory), b"foo\n");
            }

            #[tokio::test]
            async fn remove_only_incomplete_line() {
                let directory = tempdir().unwrap();

                write_log(&directory, "fo");

                let database = reopen(&directory).await;

                assert_eq!(database.get_outputs().unwrap(), Vec::<String>::new());
                assert_eq!(read_log(&directory), b"");
            }

            #[tokio::test]
            async fn remove_incomplete_line_in_incomplete_utf8() {
                let directory = tempdir().unwrap();

                write_log(
                    &directory,
                    [b"foo\n".as_slice(), &"😄".as_bytes()[..2]].concat(),
                );

                let database = reopen(&directory).await;

                assert_eq!(database.get_outputs().unwrap(), ["foo"]);
                assert_eq!(read_log(&directory), b"foo\n");
            }

            #[tokio::test]
            async fn append_line_after_incomplete_line() {
                let directory = tempdir().unwrap();

                write_log(&directory, "foo\nba");

                reopen(&directory).await.set_output("bar").unwrap();

                assert_eq!(read_log(&directory), b"foo\nbar\n");
                assert_eq!(get_outputs(&reopen(&directory).await), ["bar", "foo"]);
            }
        }
    }
}
