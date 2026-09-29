use super::utility::{
    COMPACTION_RATIO, LINE_TERMINATOR, compact_file, open_file, read_file, split_lines,
};
use crate::{infrastructure::DatabaseError, ir::BuildId, path_pool::PathPool};
use alloc::sync::Arc;
use core::{iter::successors, str};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Write},
    path::Path,
};
use tokio::{fs::create_dir_all, sync::Mutex, try_join};

const PATH_FILENAME: &str = "paths";
const INDEX_FILENAME: &str = "indices";
const RECORD_HEADER_SIZE: usize = size_of::<u64>() + size_of::<Index>();

type Index = [u8; size_of::<u32>()];

pub struct HeaderInputLog {
    state: Mutex<State>,
}

struct State {
    path_file: File,
    index_file: File,
    path_count: u32,
    indices: HashMap<Arc<str>, u32>,
    inputs: HashMap<BuildId, Vec<Arc<str>>>,
    failed: bool,
}

impl HeaderInputLog {
    pub async fn new(directory: &Path, path_pool: &PathPool) -> Result<Self, DatabaseError> {
        create_dir_all(directory).await?;

        let path_file = directory.join(PATH_FILENAME);
        let index_file = directory.join(INDEX_FILENAME);
        let (path_bytes, index_bytes) = try_join!(read_file(&path_file), read_file(&index_file))?;
        let paths = split_lines(&path_bytes)
            .map(|line| Ok(path_pool.intern(str::from_utf8(line)?)))
            .collect::<Result<Vec<_>, DatabaseError>>()?;
        let records = successors(deserialize_record(&index_bytes), |(_, _, bytes)| {
            deserialize_record(bytes)
        })
        .collect::<Vec<_>>();
        let indices = records
            .iter()
            .map(|&(id, indices, _)| (id, indices))
            .collect::<HashMap<_, _>>();
        // Paths might be lost on a system failure while records of them are not.
        let inputs = indices
            .iter()
            .filter_map(|(&id, indices)| {
                Some((
                    id,
                    indices
                        .iter()
                        .map(|&index| deserialize_input(index, &paths))
                        .collect::<Option<_>>()?,
                ))
            })
            .collect::<HashMap<_, Vec<_>>>();
        let (path_file, index_file) = try_join!(
            open_log(
                &path_file,
                path_bytes
                    .last()
                    .is_some_and(|&byte| byte != LINE_TERMINATOR)
                    .then(|| serialize_paths(&paths))
            ),
            open_log(
                &index_file,
                (inputs.len() != indices.len()
                    || !records
                        .last()
                        .map_or(index_bytes.as_slice(), |&(_, _, bytes)| bytes)
                        .is_empty()
                    || index_bytes.len()
                        > COMPACTION_RATIO
                            * inputs
                                .values()
                                .map(|inputs| {
                                    RECORD_HEADER_SIZE + size_of::<Index>() * inputs.len()
                                })
                                .sum::<usize>())
                .then(|| {
                    indices
                        .iter()
                        .filter(|(id, _)| inputs.contains_key(id))
                        .map(|(&id, indices)| serialize_record(id, indices))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .map(|records| records.concat())
            )
        )?;

        Ok(Self {
            state: State {
                path_file,
                index_file,
                path_count: paths.len().try_into()?,
                indices: paths.into_iter().zip(0..).collect(),
                inputs,
                failed: false,
            }
            .into(),
        })
    }

    pub async fn get(&self, id: BuildId) -> Vec<Arc<str>> {
        self.state
            .lock()
            .await
            .inputs
            .get(&id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn set(&self, id: BuildId, inputs: &[Arc<str>]) -> Result<(), DatabaseError> {
        // The lock keeps paths in the same order in the file and memory.
        let state = &mut *self.state.lock().await;

        if state.failed {
            return Err(DatabaseError::new("header input log failed to be written"));
        } else if let Some(path) = inputs
            .iter()
            .find(|path| path.contains(char::from(LINE_TERMINATOR)))
        {
            return Err(DatabaseError::new(format!(
                "invalid header input path: {path}"
            )));
        } else if state.inputs.get(&id).map(Vec::as_slice).unwrap_or_default() == inputs {
            return Ok(());
        }

        let mut paths = vec![];
        let indices = inputs
            .iter()
            .map(|path| {
                state
                    .indices
                    .entry(path.clone())
                    .or_insert_with(|| {
                        paths.push(path.clone());
                        state.path_count += 1;
                        state.path_count - 1
                    })
                    .to_le_bytes()
            })
            .collect::<Vec<_>>();

        if let Err(error) = state.write(id, &paths, &indices) {
            // The files and memory might have different paths.
            state.failed = true;

            return Err(error);
        }

        state.inputs.insert(id, inputs.into());

        Ok(())
    }
}

impl State {
    // Paths are written first so that records never refer to paths written later.
    fn write(
        &mut self,
        id: BuildId,
        paths: &[Arc<str>],
        indices: &[Index],
    ) -> Result<(), DatabaseError> {
        self.path_file.write_all(&serialize_paths(paths))?;
        self.index_file.write_all(&serialize_record(id, indices)?)?;

        Ok(())
    }
}

async fn open_log(path: &Path, bytes: Option<Vec<u8>>) -> Result<File, io::Error> {
    if let Some(bytes) = bytes {
        compact_file(path, bytes).await?;
    }

    open_file(path).await
}

fn serialize_paths(paths: &[Arc<str>]) -> Vec<u8> {
    paths
        .iter()
        .flat_map(|path| [path.as_bytes(), &[LINE_TERMINATOR]].concat())
        .collect()
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
    use super::*;
    use futures::future::try_join_all;
    use pretty_assertions::assert_eq;
    use std::fs::{exists, read, write};
    use tempfile::{TempDir, tempdir};
    use tokio::spawn;

    async fn open() -> (HeaderInputLog, TempDir) {
        let directory = tempdir().unwrap();

        (reopen(&directory).await, directory)
    }

    async fn reopen(directory: &TempDir) -> HeaderInputLog {
        HeaderInputLog::new(directory.path(), &Default::default())
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

    fn read_path_file(directory: &TempDir) -> String {
        String::from_utf8(read(directory.path().join(PATH_FILENAME)).unwrap()).unwrap()
    }

    fn read_index_file(directory: &TempDir) -> Vec<u8> {
        read(directory.path().join(INDEX_FILENAME)).unwrap()
    }

    fn read_records(directory: &TempDir) -> Vec<Vec<u8>> {
        let bytes = read_index_file(directory);
        let mut records = successors(deserialize_record(&bytes), |(_, _, bytes)| {
            deserialize_record(bytes)
        })
        .map(|(id, indices, _)| serialize_record(id, indices).unwrap())
        .collect::<Vec<_>>();

        records.sort();

        records
    }

    fn write_path_file(directory: &TempDir, paths: impl AsRef<[u8]>) {
        write(directory.path().join(PATH_FILENAME), paths).unwrap();
    }

    fn write_index_file(directory: &TempDir, records: &[Vec<u8>]) {
        write(directory.path().join(INDEX_FILENAME), records.concat()).unwrap();
    }

    #[tokio::test]
    async fn new() {
        let (_log, directory) = open().await;

        assert!(exists(directory.path().join("paths")).unwrap());
        assert!(exists(directory.path().join("indices")).unwrap());
    }

    #[tokio::test]
    async fn new_in_missing_directory() {
        let directory = tempdir().unwrap();

        HeaderInputLog::new(
            &directory.path().join("foo").join("bar"),
            &Default::default(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn header_inputs() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into(), "bar".into()])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)).await, ["foo".into(), "bar".into()]);
    }

    #[tokio::test]
    async fn get_no_header_inputs() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

        assert_eq!(log.get(BuildId::new(2)).await, Vec::<Arc<str>>::new());
    }

    #[tokio::test]
    async fn update_header_inputs() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into(), "bar".into()])
            .await
            .unwrap();
        log.set(BuildId::new(1), &["bar".into(), "baz".into()])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)).await, ["bar".into(), "baz".into()]);
    }

    #[tokio::test]
    async fn update_header_inputs_to_none() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
        log.set(BuildId::new(1), &[]).await.unwrap();

        assert_eq!(log.get(BuildId::new(1)).await, Vec::<Arc<str>>::new());
    }

    #[tokio::test]
    async fn set_header_inputs_of_builds() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into(), "bar".into()])
            .await
            .unwrap();
        log.set(BuildId::new(2), &["bar".into(), "baz".into()])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)).await, ["foo".into(), "bar".into()]);
        assert_eq!(log.get(BuildId::new(2)).await, ["bar".into(), "baz".into()]);
    }

    #[tokio::test]
    async fn set_duplicate_header_inputs() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into(), "foo".into()])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)).await, ["foo".into(), "foo".into()]);
    }

    #[tokio::test]
    async fn set_header_input_in_directory() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo/bar baz.h".into()])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)).await, ["foo/bar baz.h".into()]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_header_inputs_concurrently() {
        const BUILD_COUNT: u64 = 1024;
        const INPUT_COUNT: u64 = 8;
        const PATH_COUNT: u64 = 64;

        fn inputs(id: u64) -> Vec<Arc<str>> {
            (0..INPUT_COUNT)
                .map(|index| format!("{}.h", (id + index * index) % PATH_COUNT).into())
                .collect()
        }

        let (log, directory) = open().await;
        let log = Arc::new(log);

        for result in try_join_all((0..BUILD_COUNT).map(|id| {
            let log = log.clone();

            spawn(async move { log.set(BuildId::new(id), &inputs(id)).await })
        }))
        .await
        .unwrap()
        {
            result.unwrap();
        }

        for id in 0..BUILD_COUNT {
            assert_eq!(log.get(BuildId::new(id)).await, inputs(id));
        }

        drop(log);

        let log = reopen(&directory).await;

        for id in 0..BUILD_COUNT {
            assert_eq!(log.get(BuildId::new(id)).await, inputs(id));
        }
    }

    #[tokio::test]
    async fn intern_header_inputs() {
        let (log, directory) = open().await;

        log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

        drop(log);

        let path_pool = PathPool::new();
        let log = HeaderInputLog::new(directory.path(), &path_pool)
            .await
            .unwrap();

        assert!(Arc::ptr_eq(
            &log.get(BuildId::new(1)).await[0],
            &path_pool.intern("foo")
        ));
    }

    #[tokio::test]
    async fn fail_to_set_header_input_with_line_terminator() {
        let (log, directory) = open().await;

        assert!(
            log.set(BuildId::new(1), &["foo".into(), "bar\nbaz".into()])
                .await
                .is_err()
        );
        assert_eq!(log.get(BuildId::new(1)).await, Vec::<Arc<str>>::new());
        assert_eq!(read_path_file(&directory), "");
        assert_eq!(read_index_file(&directory), b"");
    }

    #[tokio::test]
    async fn fail_to_set_header_inputs_after_failed_write_of_paths() {
        let (log, directory) = open().await;

        log.state.lock().await.path_file =
            File::open(directory.path().join(PATH_FILENAME)).unwrap();

        assert!(log.set(BuildId::new(1), &["foo".into()]).await.is_err());

        log.state.lock().await.path_file = open_file(&directory.path().join(PATH_FILENAME))
            .await
            .unwrap();

        assert!(log.set(BuildId::new(2), &["bar".into()]).await.is_err());
        assert_eq!(log.get(BuildId::new(1)).await, Vec::<Arc<str>>::new());
        assert_eq!(log.get(BuildId::new(2)).await, Vec::<Arc<str>>::new());
        assert_eq!(read_path_file(&directory), "");
        assert_eq!(read_index_file(&directory), b"");
    }

    #[tokio::test]
    async fn fail_to_set_header_inputs_after_failed_write_of_record() {
        let (log, directory) = open().await;

        log.state.lock().await.index_file =
            File::open(directory.path().join(INDEX_FILENAME)).unwrap();

        assert!(log.set(BuildId::new(1), &["foo".into()]).await.is_err());

        log.state.lock().await.index_file = open_file(&directory.path().join(INDEX_FILENAME))
            .await
            .unwrap();

        assert!(log.set(BuildId::new(2), &["bar".into()]).await.is_err());
        assert_eq!(log.get(BuildId::new(1)).await, Vec::<Arc<str>>::new());
        assert_eq!(log.get(BuildId::new(2)).await, Vec::<Arc<str>>::new());
        assert_eq!(read_path_file(&directory), "foo\n");
        assert_eq!(read_index_file(&directory), b"");
    }

    mod path {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn write_paths() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            assert_eq!(read_path_file(&directory), "foo\nbar\n");
        }

        #[tokio::test]
        async fn write_path_in_utf8() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["😄".into()]).await.unwrap();

            assert_eq!(read_path_file(&directory), "😄\n");
        }

        #[tokio::test]
        async fn write_path_once() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(2), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            assert_eq!(read_path_file(&directory), "foo\nbar\n");
        }

        #[tokio::test]
        async fn write_duplicate_path_once() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(read_path_file(&directory), "foo\n");
        }

        #[tokio::test]
        async fn write_no_path_of_no_header_input() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &[]).await.unwrap();

            assert_eq!(read_path_file(&directory), "");
        }

        #[tokio::test]
        async fn reopen_paths_in_utf8() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["😄".into()]).await.unwrap();

            drop(log);

            assert_eq!(
                reopen(&directory).await.get(BuildId::new(1)).await,
                ["😄".into()]
            );
        }

        #[tokio::test]
        async fn reopen_duplicate_paths() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\nfoo\n");
            write_index_file(&directory, &[record(1, &[0, 1])]);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into(), "foo".into()]);

            log.set(BuildId::new(2), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            drop(log);

            assert_eq!(read_path_file(&directory), "foo\nfoo\nbar\n");
            assert_eq!(
                reopen(&directory).await.get(BuildId::new(2)).await,
                ["foo".into(), "bar".into()]
            );
        }

        #[tokio::test]
        async fn append_paths_after_reopen() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            drop(log);

            reopen(&directory)
                .await
                .set(BuildId::new(2), &["bar".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(read_path_file(&directory), "foo\nbar\n");
        }

        #[tokio::test]
        async fn keep_unused_path() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\nbar\n");
            write_index_file(&directory, &[record(1, &[1])]);

            reopen(&directory).await;

            assert_eq!(read_path_file(&directory), "foo\nbar\n");
        }

        #[tokio::test]
        async fn remove_incomplete_path() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\nba");
            write_index_file(&directory, &[record(1, &[0])]);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(read_path_file(&directory), "foo\n");
        }

        #[tokio::test]
        async fn remove_only_incomplete_path() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "fo");

            reopen(&directory).await;

            assert_eq!(read_path_file(&directory), "");
        }

        #[tokio::test]
        async fn remove_incomplete_path_in_incomplete_utf8() {
            let directory = tempdir().unwrap();

            write_path_file(
                &directory,
                [b"foo\n".as_slice(), &"😄".as_bytes()[..2]].concat(),
            );

            reopen(&directory).await;

            assert_eq!(read_path_file(&directory), "foo\n");
        }

        #[tokio::test]
        async fn append_path_after_incomplete_path() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\nba");

            reopen(&directory)
                .await
                .set(BuildId::new(1), &["bar".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(read_path_file(&directory), "foo\nbar\n");
            assert_eq!(
                reopen(&directory).await.get(BuildId::new(1)).await,
                ["bar".into(), "foo".into()]
            );
        }

        #[tokio::test]
        async fn fail_to_open_paths_in_invalid_utf8() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, [b'f', 0xff, b'\n']);

            assert!(
                HeaderInputLog::new(directory.path(), &Default::default())
                    .await
                    .is_err()
            );
        }
    }

    mod index {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn write_record() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            assert_eq!(
                read_index_file(&directory),
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

            let (log, directory) = open().await;

            log.set(
                BuildId::new(0x0102_0304_0506_0708),
                &(0..INPUT_COUNT)
                    .map(|index| index.to_string().into())
                    .collect::<Vec<_>>(),
            )
            .await
            .unwrap();

            let bytes = read_index_file(&directory);

            assert_eq!(
                bytes[..RECORD_HEADER_SIZE],
                [
                    [0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01].as_slice(),
                    &[0x02, 0x01, 0, 0]
                ]
                .concat()
            );
            assert_eq!(
                bytes[RECORD_HEADER_SIZE + size_of::<Index>() * (INPUT_COUNT - 2)..],
                [[0x00, 0x01, 0, 0], [0x01, 0x01, 0, 0]].concat()
            );
        }

        #[tokio::test]
        async fn write_records() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(2), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            assert_eq!(
                read_index_file(&directory),
                [record(1, &[0]), record(2, &[0, 1])].concat()
            );
        }

        #[tokio::test]
        async fn write_record_of_duplicate_header_inputs() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(read_index_file(&directory), record(1, &[0, 0]));
        }

        #[tokio::test]
        async fn write_record_of_updated_header_inputs() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(1), &["bar".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(
                read_index_file(&directory),
                [record(1, &[0]), record(1, &[1, 0])].concat()
            );
        }

        #[tokio::test]
        async fn write_record_of_no_header_input() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(1), &[]).await.unwrap();

            assert_eq!(
                read_index_file(&directory),
                [record(1, &[0]), record(1, &[])].concat()
            );
        }

        #[tokio::test]
        async fn write_no_record_of_same_header_inputs() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            assert_eq!(read_index_file(&directory), record(1, &[0]));
        }

        #[tokio::test]
        async fn write_no_record_of_no_header_input() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &[]).await.unwrap();

            assert_eq!(read_index_file(&directory), b"");
        }

        #[tokio::test]
        async fn reopen_records() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into(), "bar".into()])
                .await
                .unwrap();
            log.set(BuildId::new(2), &["bar".into(), "baz".into()])
                .await
                .unwrap();

            drop(log);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into(), "bar".into()]);
            assert_eq!(log.get(BuildId::new(2)).await, ["bar".into(), "baz".into()]);
        }

        #[tokio::test]
        async fn reopen_record_of_updated_header_inputs() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(1), &["bar".into()]).await.unwrap();

            drop(log);

            assert_eq!(
                reopen(&directory).await.get(BuildId::new(1)).await,
                ["bar".into()]
            );
        }

        #[tokio::test]
        async fn reopen_record_of_no_header_input() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(1), &[]).await.unwrap();

            drop(log);

            assert_eq!(
                reopen(&directory).await.get(BuildId::new(1)).await,
                Vec::<Arc<str>>::new()
            );
        }

        #[tokio::test]
        async fn reopen_record_with_large_id() {
            let (log, directory) = open().await;

            log.set(BuildId::new(u64::MAX), &["foo".into()])
                .await
                .unwrap();

            drop(log);

            assert_eq!(
                reopen(&directory).await.get(BuildId::new(u64::MAX)).await,
                ["foo".into()]
            );
        }

        #[tokio::test]
        async fn append_records_after_reopen() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            drop(log);

            reopen(&directory)
                .await
                .set(BuildId::new(2), &["bar".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(
                read_index_file(&directory),
                [record(1, &[0]), record(2, &[1, 0])].concat()
            );
        }

        #[tokio::test]
        async fn append_no_record_of_same_header_inputs_after_reopen() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            drop(log);

            reopen(&directory)
                .await
                .set(BuildId::new(1), &["foo".into()])
                .await
                .unwrap();

            assert_eq!(read_index_file(&directory), record(1, &[0]));
        }
    }

    mod compaction {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn compact() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\n");
            write_index_file(&directory, &vec![record(1, &[0]); 4]);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(read_index_file(&directory), record(1, &[0]));
        }

        #[tokio::test]
        async fn compact_updated_header_inputs() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\nbar\n");
            write_index_file(
                &directory,
                &[
                    record(1, &[0, 1]),
                    record(1, &[0, 1]),
                    record(1, &[0, 1]),
                    record(1, &[1, 0]),
                ],
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["bar".into(), "foo".into()]);
            assert_eq!(read_index_file(&directory), record(1, &[1, 0]));
        }

        #[tokio::test]
        async fn compact_many_builds() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\nbar\n");
            write_index_file(
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

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["bar".into()]);
            assert_eq!(log.get(BuildId::new(2)).await, ["bar".into(), "foo".into()]);
            assert_eq!(
                read_records(&directory),
                [record(1, &[1]), record(2, &[1, 0])]
            );
        }

        #[tokio::test]
        async fn compact_records_of_different_lengths() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\nbar\nbaz\n");
            write_index_file(
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

            reopen(&directory).await;

            assert_eq!(
                read_records(&directory),
                [record(1, &[0]), record(2, &[0, 1, 2])]
            );
        }

        #[tokio::test]
        async fn compact_after_appending_records() {
            let (log, directory) = open().await;

            for inputs in [["foo"], ["bar"]].into_iter().cycle().take(4) {
                log.set(BuildId::new(1), &inputs.map(From::from))
                    .await
                    .unwrap();
            }

            drop(log);

            assert_eq!(
                read_index_file(&directory),
                [
                    record(1, &[0]),
                    record(1, &[1]),
                    record(1, &[0]),
                    record(1, &[1])
                ]
                .concat()
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["bar".into()]);
            assert_eq!(read_index_file(&directory), record(1, &[1]));
        }

        #[tokio::test]
        async fn append_record_after_compaction() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\nbar\n");
            write_index_file(&directory, &vec![record(1, &[1]); 4]);

            reopen(&directory)
                .await
                .set(BuildId::new(2), &["baz".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(read_path_file(&directory), "foo\nbar\nbaz\n");
            assert_eq!(
                read_index_file(&directory),
                [record(1, &[1]), record(2, &[2, 0])].concat()
            );
        }

        #[tokio::test]
        async fn keep_log_of_optimal_size() {
            let directory = tempdir().unwrap();
            let records = [record(1, &[0]), record(2, &[1, 0])];

            write_path_file(&directory, "foo\nbar\n");
            write_index_file(&directory, &records);

            reopen(&directory).await;

            assert_eq!(read_index_file(&directory), records.concat());
        }

        #[tokio::test]
        async fn keep_log_of_compaction_ratio() {
            let directory = tempdir().unwrap();
            let records = [record(1, &[0]), record(1, &[0]), record(1, &[0])];

            write_path_file(&directory, "foo\n");
            write_index_file(&directory, &records);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(read_index_file(&directory), records.concat());
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

            write_path_file(&directory, "foo\nbar\nbaz\n");
            write_index_file(&directory, &records);

            reopen(&directory).await;

            assert_eq!(read_index_file(&directory), records.concat());
        }

        #[tokio::test]
        async fn keep_empty_log() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "");
            write_index_file(&directory, &[]);

            reopen(&directory).await;

            assert_eq!(read_path_file(&directory), "");
            assert_eq!(read_index_file(&directory), b"");
        }
    }

    mod incomplete_record {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn remove_incomplete_record() {
            for size in 1..record(2, &[0, 0]).len() {
                let directory = tempdir().unwrap();

                write_path_file(&directory, "foo\n");
                write_index_file(
                    &directory,
                    &[record(1, &[0]), record(2, &[0, 0])[..size].into()],
                );

                let log = reopen(&directory).await;

                assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
                assert_eq!(log.get(BuildId::new(2)).await, Vec::<Arc<str>>::new());
                assert_eq!(read_index_file(&directory), record(1, &[0]));
            }
        }

        #[tokio::test]
        async fn remove_only_incomplete_record() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\n");
            write_index_file(&directory, &[record(1, &[0])[..1].into()]);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, Vec::<Arc<str>>::new());
            assert_eq!(read_index_file(&directory), b"");
        }

        #[tokio::test]
        async fn remove_incomplete_record_with_large_count() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\n");
            write_index_file(
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

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)).await, Vec::<Arc<str>>::new());
            assert_eq!(read_index_file(&directory), record(1, &[0]));
        }

        #[tokio::test]
        async fn append_record_after_incomplete_record() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\n");
            write_index_file(&directory, &[record(1, &[0]), record(2, &[0])[..1].into()]);

            reopen(&directory)
                .await
                .set(BuildId::new(3), &["bar".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(
                read_index_file(&directory),
                [record(1, &[0]), record(3, &[1, 0])].concat()
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(log.get(BuildId::new(3)).await, ["bar".into(), "foo".into()]);
        }
    }

    mod missing_path {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn remove_record() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\n");
            write_index_file(&directory, &[record(1, &[0]), record(2, &[0, 1])]);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)).await, Vec::<Arc<str>>::new());
            assert_eq!(read_path_file(&directory), "foo\n");
            assert_eq!(read_index_file(&directory), record(1, &[0]));
        }

        #[tokio::test]
        async fn remove_record_of_updated_header_inputs() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\n");
            write_index_file(&directory, &[record(1, &[0]), record(1, &[1])]);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, Vec::<Arc<str>>::new());
            assert_eq!(read_index_file(&directory), b"");
        }

        #[tokio::test]
        async fn keep_record_updated_from_one_with_missing_path() {
            let directory = tempdir().unwrap();
            let records = [record(1, &[1]), record(1, &[0])];

            write_path_file(&directory, "foo\n");
            write_index_file(&directory, &records);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(read_index_file(&directory), records.concat());
        }

        #[tokio::test]
        async fn remove_record_without_any_path() {
            let directory = tempdir().unwrap();

            write_index_file(&directory, &[record(1, &[0])]);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, Vec::<Arc<str>>::new());
            assert_eq!(read_index_file(&directory), b"");
        }

        #[tokio::test]
        async fn append_path_after_removing_record() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, "foo\n");
            write_index_file(&directory, &[record(1, &[0]), record(2, &[1])]);

            reopen(&directory)
                .await
                .set(BuildId::new(3), &["bar".into()])
                .await
                .unwrap();

            assert_eq!(read_path_file(&directory), "foo\nbar\n");
            assert_eq!(
                read_index_file(&directory),
                [record(1, &[0]), record(3, &[1])].concat()
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)).await, Vec::<Arc<str>>::new());
            assert_eq!(log.get(BuildId::new(3)).await, ["bar".into()]);
        }
    }
}
