use super::super::utility::{COMPACTION_RATIO, open_log};
use crate::{infrastructure::DatabaseError, ir::BuildId};
use alloc::sync::Arc;
use core::iter::successors;
use rapidhash::{RapidHashMap, fast::RandomState};
use scc::{HashIndex, hash_index::Entry};
use std::{fs::File, io::Write, path::Path};

const RECORD_HEADER_SIZE: usize = size_of::<u64>() + size_of::<Index>();

type Index = [u8; size_of::<u32>()];

pub struct IndexLog {
    file: File,
    inputs: HashIndex<BuildId, Vec<Arc<str>>, RandomState>,
}

impl IndexLog {
    pub async fn new(path: &Path, bytes: &[u8], paths: &[Arc<str>]) -> Result<Self, DatabaseError> {
        let records = successors(deserialize_record(bytes), |(_, _, bytes)| {
            deserialize_record(bytes)
        })
        .collect::<Vec<_>>();
        let mut indices = records
            .iter()
            .map(|&(id, indices, _)| (id, indices))
            .collect::<RapidHashMap<_, _>>();
        let count = indices.len();
        let inputs = HashIndex::with_capacity_and_hasher(count, Default::default());

        // Paths might be lost on a system failure while records of them are not.
        indices.retain(|&id, indices| {
            indices
                .iter()
                .map(|&index| deserialize_input(index, paths))
                .collect::<Option<_>>()
                .is_some_and(|paths| inputs.insert_sync(id, paths).is_ok())
        });

        Ok(Self {
            file: open_log(
                path,
                (indices.len() != count
                    || !records
                        .last()
                        .map_or(bytes, |&(_, _, bytes)| bytes)
                        .is_empty()
                    || bytes.len()
                        > COMPACTION_RATIO
                            * indices
                                .values()
                                .map(|indices| RECORD_HEADER_SIZE + size_of_val(*indices))
                                .sum::<usize>())
                .then(|| {
                    indices
                        .iter()
                        .map(|(&id, indices)| serialize_record(id, indices))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .map(|records| records.concat()),
            )?,
            inputs,
        })
    }

    pub fn get(&self, id: BuildId) -> Vec<Arc<str>> {
        self.inputs
            .peek_with(&id, |_, inputs| inputs.clone())
            .unwrap_or_default()
    }

    pub async fn set(
        &self,
        id: BuildId,
        inputs: &[Arc<str>],
        indices: &[u32],
    ) -> Result<(), DatabaseError> {
        (&self.file).write_all(&serialize_record(
            id,
            &indices
                .iter()
                .map(|index| index.to_le_bytes())
                .collect::<Vec<_>>(),
        )?)?;

        match self.inputs.entry_async(id).await {
            Entry::Occupied(mut entry) => entry.update(inputs.into()),
            Entry::Vacant(entry) => {
                entry.insert_entry(inputs.into());
            }
        }

        Ok(())
    }
}

fn serialize_record(id: BuildId, indices: &[Index]) -> Result<Vec<u8>, DatabaseError> {
    Ok([
        id.to_bytes().as_slice(),
        &u32::try_from(indices.len())?.to_le_bytes(),
        indices.as_flattened(),
    ]
    .concat())
}

fn deserialize_record(bytes: &[u8]) -> Option<(BuildId, &[Index], &[u8])> {
    let (id, bytes) = bytes.split_first_chunk()?;
    let (count, bytes) = bytes.split_first_chunk()?;
    let (indices, bytes) = bytes.split_at_checked(
        size_of::<Index>().checked_mul(u32::from_le_bytes(*count).try_into().ok()?)?,
    )?;

    Some((BuildId::from_bytes(*id), indices.as_chunks().0, bytes))
}

fn deserialize_input(index: Index, paths: &[Arc<str>]) -> Option<Arc<str>> {
    paths
        .get(usize::try_from(u32::from_le_bytes(index)).ok()?)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::{super::super::utility::open_file, *};
    use futures::future::try_join_all;
    use pretty_assertions::assert_eq;
    use std::fs::{exists, read, write};
    use tempfile::{TempDir, tempdir};
    use tokio::spawn;

    const FILENAME: &str = "log";

    async fn open(paths: &[&str]) -> (IndexLog, TempDir) {
        let directory = tempdir().unwrap();

        (reopen(&directory, paths).await, directory)
    }

    async fn reopen(directory: &TempDir, paths: &[&str]) -> IndexLog {
        let path = directory.path().join(FILENAME);

        IndexLog::new(
            &path,
            &read(&path).unwrap_or_default(),
            &paths.iter().copied().map(From::from).collect::<Vec<_>>(),
        )
        .await
        .unwrap()
    }

    fn record(id: u64, indices: &[u32]) -> Vec<u8> {
        serialize_record(
            BuildId::new(id),
            &indices
                .iter()
                .map(|index| index.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    fn read_log(directory: &TempDir) -> Vec<u8> {
        read(directory.path().join(FILENAME)).unwrap()
    }

    fn read_records(directory: &TempDir) -> Vec<Vec<u8>> {
        let bytes = read_log(directory);
        let mut records = successors(deserialize_record(&bytes), |(_, _, bytes)| {
            deserialize_record(bytes)
        })
        .map(|(id, indices, _)| serialize_record(id, indices).unwrap())
        .collect::<Vec<_>>();

        records.sort();

        records
    }

    fn write_log(directory: &TempDir, records: &[Vec<u8>]) {
        write(directory.path().join(FILENAME), records.concat()).unwrap();
    }

    #[tokio::test]
    async fn new() {
        let (_log, directory) = open(&[]).await;

        assert!(exists(directory.path().join(FILENAME)).unwrap());
    }

    #[tokio::test]
    async fn header_inputs() {
        let (log, _directory) = open(&[]).await;

        log.set(BuildId::new(1), &["foo".into(), "bar".into()], &[0, 1])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)), ["foo".into(), "bar".into()]);
    }

    #[tokio::test]
    async fn get_no_header_inputs() {
        let (log, _directory) = open(&[]).await;

        log.set(BuildId::new(1), &["foo".into()], &[0])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(2)), Vec::<Arc<str>>::new());
    }

    #[tokio::test]
    async fn update_header_inputs() {
        let (log, _directory) = open(&[]).await;

        log.set(BuildId::new(1), &["foo".into(), "bar".into()], &[0, 1])
            .await
            .unwrap();
        log.set(BuildId::new(1), &["bar".into(), "baz".into()], &[1, 2])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)), ["bar".into(), "baz".into()]);
    }

    #[tokio::test]
    async fn update_header_inputs_to_none() {
        let (log, _directory) = open(&[]).await;

        log.set(BuildId::new(1), &["foo".into()], &[0])
            .await
            .unwrap();
        log.set(BuildId::new(1), &[], &[]).await.unwrap();

        assert_eq!(log.get(BuildId::new(1)), Vec::<Arc<str>>::new());
    }

    #[tokio::test]
    async fn set_header_inputs_of_builds() {
        let (log, _directory) = open(&[]).await;

        log.set(BuildId::new(1), &["foo".into(), "bar".into()], &[0, 1])
            .await
            .unwrap();
        log.set(BuildId::new(2), &["bar".into(), "baz".into()], &[1, 2])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)), ["foo".into(), "bar".into()]);
        assert_eq!(log.get(BuildId::new(2)), ["bar".into(), "baz".into()]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_header_inputs_concurrently() {
        const BUILD_COUNT: u64 = 1024;
        const PATHS: [&str; 3] = ["foo", "bar", "baz"];

        fn indices(id: u64) -> [u32; 2] {
            [id % 3, (id + 1) % 3].map(|index| index as u32)
        }

        fn inputs(id: u64) -> [Arc<str>; 2] {
            indices(id).map(|index| PATHS[index as usize].into())
        }

        let (log, directory) = open(&PATHS).await;
        let log = Arc::new(log);

        for result in try_join_all((0..BUILD_COUNT).map(|id| {
            let log = log.clone();

            spawn(async move { log.set(BuildId::new(id), &inputs(id), &indices(id)).await })
        }))
        .await
        .unwrap()
        {
            result.unwrap();
        }

        drop(log);

        let log = reopen(&directory, &PATHS).await;

        for id in 0..BUILD_COUNT {
            assert_eq!(log.get(BuildId::new(id)), inputs(id));
        }
    }

    #[tokio::test]
    async fn write_record() {
        let (log, directory) = open(&[]).await;

        log.set(BuildId::new(1), &["foo".into(), "bar".into()], &[0, 1])
            .await
            .unwrap();

        assert_eq!(
            read_log(&directory),
            [
                [1, 0, 0, 0, 0, 0, 0, 0].as_slice(),
                &[2, 0, 0, 0],
                &[0, 0, 0, 0],
                &[1, 0, 0, 0]
            ]
            .concat()
        );
    }

    #[tokio::test]
    async fn write_record_in_little_endian() {
        const INPUT_COUNT: usize = 0x102;

        let (log, directory) = open(&[]).await;

        log.set(
            BuildId::new(0x0102_0304_0506_0708),
            &vec!["foo".into(); INPUT_COUNT],
            &vec![0x1112_1314; INPUT_COUNT],
        )
        .await
        .unwrap();

        assert_eq!(
            read_log(&directory),
            [
                [0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01].as_slice(),
                &[0x02, 0x01, 0, 0],
                &[0x14, 0x13, 0x12, 0x11].repeat(INPUT_COUNT)
            ]
            .concat()
        );
    }

    #[tokio::test]
    async fn write_record_of_no_header_input() {
        let (log, directory) = open(&[]).await;

        log.set(BuildId::new(1), &[], &[]).await.unwrap();

        assert_eq!(
            read_log(&directory),
            [[1, 0, 0, 0, 0, 0, 0, 0].as_slice(), &[0, 0, 0, 0]].concat()
        );
    }

    #[tokio::test]
    async fn write_records() {
        let (log, directory) = open(&[]).await;

        log.set(BuildId::new(1), &["foo".into()], &[0])
            .await
            .unwrap();
        log.set(BuildId::new(2), &["foo".into(), "bar".into()], &[0, 1])
            .await
            .unwrap();
        log.set(BuildId::new(1), &["bar".into()], &[1])
            .await
            .unwrap();

        assert_eq!(
            read_log(&directory),
            [record(1, &[0]), record(2, &[0, 1]), record(1, &[1])].concat()
        );
    }

    #[tokio::test]
    async fn reopen_log() {
        let directory = tempdir().unwrap();

        write_log(&directory, &[record(1, &[0, 1]), record(2, &[1, 2])]);

        let log = reopen(&directory, &["foo", "bar", "baz"]).await;

        assert_eq!(log.get(BuildId::new(1)), ["foo".into(), "bar".into()]);
        assert_eq!(log.get(BuildId::new(2)), ["bar".into(), "baz".into()]);
    }

    #[tokio::test]
    async fn reopen_log_after_setting_header_inputs() {
        let (log, directory) = open(&[]).await;

        log.set(BuildId::new(1), &["foo".into(), "bar".into()], &[0, 1])
            .await
            .unwrap();

        drop(log);

        assert_eq!(
            reopen(&directory, &["foo", "bar"])
                .await
                .get(BuildId::new(1)),
            ["foo".into(), "bar".into()]
        );
    }

    #[tokio::test]
    async fn reopen_log_with_updated_header_inputs() {
        let directory = tempdir().unwrap();

        write_log(&directory, &[record(1, &[0]), record(1, &[1])]);

        assert_eq!(
            reopen(&directory, &["foo", "bar"])
                .await
                .get(BuildId::new(1)),
            ["bar".into()]
        );
    }

    #[tokio::test]
    async fn reopen_log_with_no_header_input() {
        let directory = tempdir().unwrap();

        write_log(&directory, &[record(1, &[0]), record(1, &[])]);

        assert_eq!(
            reopen(&directory, &["foo"]).await.get(BuildId::new(1)),
            Vec::<Arc<str>>::new()
        );
    }

    #[tokio::test]
    async fn reopen_log_with_duplicate_header_inputs() {
        let directory = tempdir().unwrap();

        write_log(&directory, &[record(1, &[0, 0])]);

        assert_eq!(
            reopen(&directory, &["foo"]).await.get(BuildId::new(1)),
            ["foo".into(), "foo".into()]
        );
    }

    #[tokio::test]
    async fn reopen_log_with_large_id() {
        let directory = tempdir().unwrap();

        write_log(&directory, &[record(u64::MAX, &[0])]);

        assert_eq!(
            reopen(&directory, &["foo"])
                .await
                .get(BuildId::new(u64::MAX)),
            ["foo".into()]
        );
    }

    #[tokio::test]
    async fn append_records_after_reopen() {
        let directory = tempdir().unwrap();

        write_log(&directory, &[record(1, &[0])]);

        reopen(&directory, &["foo"])
            .await
            .set(BuildId::new(2), &["bar".into(), "foo".into()], &[1, 0])
            .await
            .unwrap();

        assert_eq!(
            read_log(&directory),
            [record(1, &[0]), record(2, &[1, 0])].concat()
        );
    }

    #[tokio::test]
    async fn fail_to_open_log_in_missing_directory() {
        let directory = tempdir().unwrap();

        assert!(
            IndexLog::new(&directory.path().join("foo").join(FILENAME), &[], &[])
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn fail_to_set_header_inputs_on_failed_write() {
        let (mut log, directory) = open(&[]).await;

        log.file = File::open(directory.path().join(FILENAME)).unwrap();

        assert!(
            log.set(BuildId::new(1), &["foo".into()], &[0])
                .await
                .is_err()
        );
        assert_eq!(log.get(BuildId::new(1)), Vec::<Arc<str>>::new());
        assert_eq!(read_log(&directory), b"");
    }

    #[tokio::test]
    async fn set_header_inputs_after_failed_write() {
        let (mut log, directory) = open(&[]).await;

        log.file = File::open(directory.path().join(FILENAME)).unwrap();

        assert!(
            log.set(BuildId::new(1), &["foo".into()], &[0])
                .await
                .is_err()
        );

        log.file = open_file(&directory.path().join(FILENAME)).unwrap();
        log.set(BuildId::new(2), &["bar".into(), "foo".into()], &[1, 0])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)), Vec::<Arc<str>>::new());
        assert_eq!(log.get(BuildId::new(2)), ["bar".into(), "foo".into()]);
        assert_eq!(read_log(&directory), record(2, &[1, 0]));
    }

    mod compaction {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn compact() {
            let directory = tempdir().unwrap();

            write_log(&directory, &vec![record(1, &[0]); 4]);

            let log = reopen(&directory, &["foo"]).await;

            assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
            assert_eq!(read_log(&directory), record(1, &[0]));
        }

        #[tokio::test]
        async fn compact_updated_header_inputs() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                &[
                    record(1, &[0, 1]),
                    record(1, &[0, 1]),
                    record(1, &[0, 1]),
                    record(1, &[1, 0]),
                ],
            );

            let log = reopen(&directory, &["foo", "bar"]).await;

            assert_eq!(log.get(BuildId::new(1)), ["bar".into(), "foo".into()]);
            assert_eq!(read_log(&directory), record(1, &[1, 0]));
        }

        #[tokio::test]
        async fn compact_many_builds() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                &[
                    record(1, &[0]),
                    record(2, &[1, 0]),
                    record(1, &[0]),
                    record(2, &[1, 0]),
                    record(1, &[0]),
                    record(2, &[1, 0]),
                    record(1, &[1]),
                ],
            );

            let log = reopen(&directory, &["foo", "bar"]).await;

            assert_eq!(log.get(BuildId::new(1)), ["bar".into()]);
            assert_eq!(log.get(BuildId::new(2)), ["bar".into(), "foo".into()]);
            assert_eq!(
                read_records(&directory),
                [record(1, &[1]), record(2, &[1, 0])]
            );
        }

        #[tokio::test]
        async fn compact_records_of_different_lengths() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                &[
                    record(1, &[0]),
                    record(2, &[0, 1, 2]),
                    record(2, &[0, 1, 2]),
                    record(2, &[0, 1, 2]),
                    record(2, &[0, 1, 2]),
                    record(2, &[0, 1, 2]),
                ],
            );

            reopen(&directory, &["foo", "bar", "baz"]).await;

            assert_eq!(
                read_records(&directory),
                [record(1, &[0]), record(2, &[0, 1, 2])]
            );
        }

        #[tokio::test]
        async fn compact_after_appending_records() {
            let (log, directory) = open(&[]).await;

            for index in [0, 1, 0, 1] {
                log.set(BuildId::new(1), &["foo".into()], &[index])
                    .await
                    .unwrap();
            }

            drop(log);

            assert_eq!(
                read_log(&directory),
                [
                    record(1, &[0]),
                    record(1, &[1]),
                    record(1, &[0]),
                    record(1, &[1])
                ]
                .concat()
            );

            let log = reopen(&directory, &["foo", "bar"]).await;

            assert_eq!(log.get(BuildId::new(1)), ["bar".into()]);
            assert_eq!(read_log(&directory), record(1, &[1]));
        }

        #[tokio::test]
        async fn append_record_after_compaction() {
            let directory = tempdir().unwrap();

            write_log(&directory, &vec![record(1, &[1]); 4]);

            reopen(&directory, &["foo", "bar"])
                .await
                .set(BuildId::new(2), &["baz".into(), "foo".into()], &[2, 0])
                .await
                .unwrap();

            assert_eq!(
                read_log(&directory),
                [record(1, &[1]), record(2, &[2, 0])].concat()
            );
        }

        #[tokio::test]
        async fn keep_log_of_optimal_size() {
            let directory = tempdir().unwrap();
            let records = [record(1, &[0]), record(2, &[1, 0])];

            write_log(&directory, &records);

            reopen(&directory, &["foo", "bar"]).await;

            assert_eq!(read_log(&directory), records.concat());
        }

        #[tokio::test]
        async fn keep_log_of_compaction_ratio() {
            let directory = tempdir().unwrap();
            let records = [record(1, &[0]), record(1, &[0]), record(1, &[0])];

            write_log(&directory, &records);

            let log = reopen(&directory, &["foo"]).await;

            assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
            assert_eq!(read_log(&directory), records.concat());
        }

        #[tokio::test]
        async fn keep_log_of_compaction_ratio_with_records_of_different_lengths() {
            let directory = tempdir().unwrap();
            let records = [
                record(1, &[0]),
                record(1, &[0]),
                record(1, &[0]),
                record(1, &[0]),
                record(1, &[0]),
                record(1, &[0]),
                record(2, &[0, 1, 2]),
            ];

            write_log(&directory, &records);

            reopen(&directory, &["foo", "bar", "baz"]).await;

            assert_eq!(read_log(&directory), records.concat());
        }

        #[tokio::test]
        async fn keep_empty_log() {
            let directory = tempdir().unwrap();

            write_log(&directory, &[]);

            reopen(&directory, &[]).await;

            assert_eq!(read_log(&directory), b"");
        }
    }

    mod incomplete_record {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn remove_incomplete_record() {
            for size in 1..record(2, &[0, 0]).len() {
                let directory = tempdir().unwrap();

                write_log(
                    &directory,
                    &[record(1, &[0]), record(2, &[0, 0])[..size].into()],
                );

                let log = reopen(&directory, &["foo"]).await;

                assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
                assert_eq!(log.get(BuildId::new(2)), Vec::<Arc<str>>::new());
                assert_eq!(read_log(&directory), record(1, &[0]));
            }
        }

        #[tokio::test]
        async fn remove_only_incomplete_record() {
            let directory = tempdir().unwrap();

            write_log(&directory, &[record(1, &[0])[..1].into()]);

            let log = reopen(&directory, &["foo"]).await;

            assert_eq!(log.get(BuildId::new(1)), Vec::<Arc<str>>::new());
            assert_eq!(read_log(&directory), b"");
        }

        #[tokio::test]
        async fn remove_incomplete_record_with_large_count() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                &[
                    record(1, &[0]),
                    [
                        BuildId::new(2).to_bytes().as_slice(),
                        &u32::MAX.to_le_bytes(),
                        &[0; 4],
                    ]
                    .concat(),
                ],
            );

            let log = reopen(&directory, &["foo"]).await;

            assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)), Vec::<Arc<str>>::new());
            assert_eq!(read_log(&directory), record(1, &[0]));
        }

        #[tokio::test]
        async fn append_record_after_incomplete_record() {
            let directory = tempdir().unwrap();

            write_log(&directory, &[record(1, &[0]), record(2, &[0])[..1].into()]);

            reopen(&directory, &["foo"])
                .await
                .set(BuildId::new(3), &["bar".into(), "foo".into()], &[1, 0])
                .await
                .unwrap();

            assert_eq!(
                read_log(&directory),
                [record(1, &[0]), record(3, &[1, 0])].concat()
            );

            let log = reopen(&directory, &["foo", "bar"]).await;

            assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
            assert_eq!(log.get(BuildId::new(3)), ["bar".into(), "foo".into()]);
        }
    }

    mod missing_path {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn remove_record() {
            let directory = tempdir().unwrap();

            write_log(&directory, &[record(1, &[0]), record(2, &[0, 1])]);

            let log = reopen(&directory, &["foo"]).await;

            assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)), Vec::<Arc<str>>::new());
            assert_eq!(read_log(&directory), record(1, &[0]));
        }

        #[tokio::test]
        async fn remove_record_of_updated_header_inputs() {
            let directory = tempdir().unwrap();

            write_log(&directory, &[record(1, &[0]), record(1, &[1])]);

            let log = reopen(&directory, &["foo"]).await;

            assert_eq!(log.get(BuildId::new(1)), Vec::<Arc<str>>::new());
            assert_eq!(read_log(&directory), b"");
        }

        #[tokio::test]
        async fn remove_record_without_any_path() {
            let directory = tempdir().unwrap();

            write_log(&directory, &[record(1, &[0])]);

            let log = reopen(&directory, &[]).await;

            assert_eq!(log.get(BuildId::new(1)), Vec::<Arc<str>>::new());
            assert_eq!(read_log(&directory), b"");
        }

        #[tokio::test]
        async fn keep_record_updated_from_one_with_missing_path() {
            let directory = tempdir().unwrap();
            let records = [record(1, &[1]), record(1, &[0])];

            write_log(&directory, &records);

            let log = reopen(&directory, &["foo"]).await;

            assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
            assert_eq!(read_log(&directory), records.concat());
        }

        #[tokio::test]
        async fn append_record_after_removing_record() {
            let directory = tempdir().unwrap();

            write_log(&directory, &[record(1, &[0]), record(2, &[1])]);

            reopen(&directory, &["foo"])
                .await
                .set(BuildId::new(3), &["bar".into()], &[1])
                .await
                .unwrap();

            assert_eq!(
                read_log(&directory),
                [record(1, &[0]), record(3, &[1])].concat()
            );

            let log = reopen(&directory, &["foo", "bar"]).await;

            assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)), Vec::<Arc<str>>::new());
            assert_eq!(log.get(BuildId::new(3)), ["bar".into()]);
        }
    }
}
