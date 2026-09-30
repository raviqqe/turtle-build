use super::utility::{COMPACTION_RATIO, compact_file, open_file, read_file};
use crate::{build_hash::BuildHash, infrastructure::DatabaseError, ir::BuildId};
use scc::{Guard, HashIndex, hash_index::Entry};
use std::{fs::File, io::Write, path::Path};

type Record = [[u8; size_of::<u64>()]; 3];

pub struct HashLog {
    file: File,
    hashes: HashIndex<BuildId, BuildHash>,
}

impl HashLog {
    pub async fn new(path: &Path) -> Result<Self, DatabaseError> {
        let bytes = read_file(path)?;
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

            compact_file(path, bytes)?;
        }

        Ok(Self {
            file: open_file(path)?,
            hashes,
        })
    }

    pub fn get(&self, id: BuildId) -> Option<BuildHash> {
        self.hashes.peek_with(&id, |_, hash| *hash)
    }

    pub async fn set(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError> {
        (&self.file).write_all(serialize(id, hash).as_flattened())?;

        match self.hashes.entry_async(id).await {
            Entry::Occupied(mut entry) => entry.update(hash),
            Entry::Vacant(entry) => {
                entry.insert_entry(hash);
            }
        }

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;
    use futures::future::try_join_all;
    use pretty_assertions::assert_eq;
    use std::fs::{exists, read, write};
    use tempfile::{TempDir, tempdir};
    use tokio::spawn;

    const FILENAME: &str = "log";

    async fn open() -> (HashLog, TempDir) {
        let directory = tempdir().unwrap();

        (reopen(&directory).await, directory)
    }

    async fn reopen(directory: &TempDir) -> HashLog {
        HashLog::new(&directory.path().join(FILENAME))
            .await
            .unwrap()
    }

    fn read_log(directory: &TempDir) -> Vec<u8> {
        read(directory.path().join(FILENAME)).unwrap()
    }

    fn write_log(directory: &TempDir, records: &[Record]) {
        write(
            directory.path().join(FILENAME),
            records.as_flattened().as_flattened(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn new() {
        let (_log, directory) = open().await;

        assert!(exists(directory.path().join(FILENAME)).unwrap());
    }

    #[tokio::test]
    async fn hash() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(0)), Some(BuildHash::new(1, 2)));
    }

    #[tokio::test]
    async fn get_no_hash() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)), None);
    }

    #[tokio::test]
    async fn update_hash() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();
        log.set(BuildId::new(0), BuildHash::new(3, 4))
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(0)), Some(BuildHash::new(3, 4)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_hashes_concurrently() {
        const BUILD_COUNT: u64 = 1024;

        let (log, directory) = open().await;
        let log = Arc::new(log);

        for result in try_join_all((0..BUILD_COUNT).map(|id| {
            let log = log.clone();

            spawn(async move {
                log.set(BuildId::new(id), BuildHash::new(id + 1, id + 2))
                    .await
            })
        }))
        .await
        .unwrap()
        {
            result.unwrap();
        }

        drop(log);

        let log = reopen(&directory).await;

        for id in 0..BUILD_COUNT {
            assert_eq!(
                log.get(BuildId::new(id)),
                Some(BuildHash::new(id + 1, id + 2))
            );
        }
    }

    #[tokio::test]
    async fn write_record() {
        let (log, directory) = open().await;

        log.set(BuildId::new(1), BuildHash::new(2, 3))
            .await
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
        let (log, directory) = open().await;

        log.set(
            BuildId::new(0x0102_0304_0506_0708),
            BuildHash::new(0x1112_1314_1516_1718, 0x2122_2324_2526_2728),
        )
        .await
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
        let (log, directory) = open().await;

        log.set(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();
        log.set(BuildId::new(3), BuildHash::new(4, 5))
            .await
            .unwrap();
        log.set(BuildId::new(0), BuildHash::new(6, 7))
            .await
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
        let (log, directory) = open().await;

        log.set(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();
        log.set(BuildId::new(3), BuildHash::new(4, 5))
            .await
            .unwrap();

        drop(log);

        let log = reopen(&directory).await;

        assert_eq!(log.get(BuildId::new(0)), Some(BuildHash::new(1, 2)));
        assert_eq!(log.get(BuildId::new(3)), Some(BuildHash::new(4, 5)));
    }

    #[tokio::test]
    async fn reopen_log_with_updated_hash() {
        let (log, directory) = open().await;

        log.set(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();
        log.set(BuildId::new(0), BuildHash::new(3, 4))
            .await
            .unwrap();

        drop(log);

        assert_eq!(
            reopen(&directory).await.get(BuildId::new(0)),
            Some(BuildHash::new(3, 4))
        );
    }

    #[tokio::test]
    async fn append_records_after_reopen() {
        let (log, directory) = open().await;

        log.set(BuildId::new(0), BuildHash::new(1, 2))
            .await
            .unwrap();

        drop(log);

        reopen(&directory)
            .await
            .set(BuildId::new(3), BuildHash::new(4, 5))
            .await
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

    #[tokio::test]
    async fn fail_to_open_log_in_missing_directory() {
        let directory = tempdir().unwrap();

        assert!(
            HashLog::new(&directory.path().join("foo").join(FILENAME))
                .await
                .is_err()
        );
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

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(0)), Some(BuildHash::new(7, 8)));
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

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(0)), Some(BuildHash::new(7, 8)));
            assert_eq!(log.get(BuildId::new(10)), Some(BuildHash::new(15, 16)));

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
            let (log, directory) = open().await;

            for hash in 0..4 {
                log.set(BuildId::new(0), BuildHash::new(hash, hash))
                    .await
                    .unwrap();
            }

            drop(log);

            assert_eq!(read_log(&directory).len(), 4 * size_of::<Record>());

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(0)), Some(BuildHash::new(3, 3)));
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
                .set(BuildId::new(9), BuildHash::new(10, 11))
                .await
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

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(0)), Some(BuildHash::new(5, 6)));
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
                directory.path().join(FILENAME),
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

                let log = reopen(&directory).await;

                assert_eq!(log.get(BuildId::new(0)), Some(BuildHash::new(1, 2)));
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
                .set(BuildId::new(3), BuildHash::new(4, 5))
                .await
                .unwrap();

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(0)), Some(BuildHash::new(1, 2)));
            assert_eq!(log.get(BuildId::new(3)), Some(BuildHash::new(4, 5)));
        }
    }
}
