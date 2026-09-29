use super::utility::{
    COLUMN_SEPARATOR, COMPACTION_RATIO, LINE_TERMINATOR, compact_file, open_file, read_file,
    split_lines,
};
use crate::{infrastructure::DatabaseError, ir::BuildId, path_pool::PathPool};
use alloc::sync::Arc;
use core::str;
use itertools::Itertools;
use std::{collections::HashMap, fs::File, io::Write, path::Path};
use tokio::sync::Mutex;

const ID_RADIX: u32 = 16;

pub struct HeaderInputLog {
    state: Mutex<State>,
}

struct State {
    file: File,
    path_count: usize,
    indices: HashMap<Arc<str>, usize>,
    inputs: HashMap<BuildId, Vec<Arc<str>>>,
    failed: bool,
}

impl HeaderInputLog {
    pub async fn new(path: &Path, path_pool: &PathPool) -> Result<Self, DatabaseError> {
        let bytes = read_file(path).await?;
        let mut paths = vec![];
        let mut records = HashMap::new();

        for line in split_lines(&bytes) {
            let line = str::from_utf8(line)?;

            if let Some((id, indices)) = line.split_once(char::from(COLUMN_SEPARATOR)) {
                records.insert(deserialize_id(id)?, (line, indices));
            } else {
                paths.push(path_pool.intern(line));
            }
        }

        let inputs = records
            .iter()
            .map(|(&id, (_, indices))| Ok((id, deserialize_inputs(indices, &paths)?)))
            .collect::<Result<_, DatabaseError>>()?;
        let lines = || {
            paths
                .iter()
                .map(AsRef::as_ref)
                .chain(records.values().map(|&(line, _)| line))
        };

        if bytes.last().is_some_and(|&byte| byte != LINE_TERMINATOR)
            || bytes.len()
                > COMPACTION_RATIO
                    * lines()
                        .map(|line| line.len() + size_of_val(&LINE_TERMINATOR))
                        .sum::<usize>()
        {
            compact_file(path, serialize_lines(lines())).await?;
        }

        Ok(Self {
            state: State {
                file: open_file(path).await?,
                path_count: paths.len(),
                indices: paths
                    .into_iter()
                    .enumerate()
                    .map(|(index, path)| (path, index))
                    .collect(),
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
        // The lock keeps path lines in the same order in the file and memory.
        let state = &mut *self.state.lock().await;

        if state.failed {
            return Err(DatabaseError::new("header input log failed to be written"));
        } else if let Some(path) = inputs
            .iter()
            .find(|path| path.contains([COLUMN_SEPARATOR, LINE_TERMINATOR].map(char::from)))
        {
            return Err(DatabaseError::new(format!(
                "invalid header input path: {path}"
            )));
        } else if state.inputs.get(&id).map(Vec::as_slice).unwrap_or_default() == inputs {
            return Ok(());
        }

        let mut paths = vec![];
        let record = [
            serialize_id(id),
            inputs
                .iter()
                .map(|path| {
                    *state.indices.entry(path.clone()).or_insert_with(|| {
                        paths.push(path.as_ref());
                        state.path_count += 1;
                        state.path_count - 1
                    })
                })
                .join(str::from_utf8(&[COLUMN_SEPARATOR])?),
        ]
        .join(str::from_utf8(&[COLUMN_SEPARATOR])?);

        if let Err(error) = state
            .file
            .write_all(&serialize_lines(paths.into_iter().chain([record.as_str()])))
        {
            // The file and memory might have different path lines.
            state.failed = true;

            return Err(error.into());
        }

        state.inputs.insert(id, inputs.into());

        Ok(())
    }
}

fn serialize_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<u8> {
    lines
        .flat_map(|line| [line.as_bytes(), &[LINE_TERMINATOR]].concat())
        .collect()
}

fn serialize_id(id: BuildId) -> String {
    format!("{:x}", u64::from_le_bytes(id.to_bytes()))
}

fn deserialize_id(id: &str) -> Result<BuildId, DatabaseError> {
    Ok(BuildId::new(u64::from_str_radix(id, ID_RADIX)?))
}

fn deserialize_inputs(indices: &str, paths: &[Arc<str>]) -> Result<Vec<Arc<str>>, DatabaseError> {
    indices
        .split_terminator(char::from(COLUMN_SEPARATOR))
        .map(|index| {
            paths
                .get(index.parse::<usize>()?)
                .cloned()
                .ok_or_else(|| DatabaseError::new("path index out of range in header input log"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::try_join_all;
    use pretty_assertions::assert_eq;
    use std::fs::{exists, read, write};
    use tempfile::{TempDir, tempdir};
    use tokio::spawn;

    const FILENAME: &str = "log";
    const NUL: char = '\0';

    async fn open() -> (HeaderInputLog, TempDir) {
        let directory = tempdir().unwrap();

        (reopen(&directory).await, directory)
    }

    async fn reopen(directory: &TempDir) -> HeaderInputLog {
        HeaderInputLog::new(&directory.path().join(FILENAME), &Default::default())
            .await
            .unwrap()
    }

    fn read_log(directory: &TempDir) -> String {
        String::from_utf8(read(directory.path().join(FILENAME)).unwrap()).unwrap()
    }

    fn read_lines(directory: &TempDir) -> Vec<String> {
        let mut lines = read_log(directory)
            .split_inclusive('\n')
            .map(From::from)
            .collect::<Vec<_>>();

        lines.sort();

        lines
    }

    fn write_log(directory: &TempDir, log: impl AsRef<[u8]>) {
        write(directory.path().join(FILENAME), log).unwrap();
    }

    #[tokio::test]
    async fn new() {
        let (_log, directory) = open().await;

        assert!(exists(directory.path().join(FILENAME)).unwrap());
    }

    #[tokio::test]
    async fn header_inputs() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into(), "bar".into()])
            .await
            .unwrap();

        assert_eq!(
            log.get(BuildId::new(1)).await,
            ["foo".into(), "bar".into()]
        );
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

        assert_eq!(
            log.get(BuildId::new(1)).await,
            ["bar".into(), "baz".into()]
        );
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

        assert_eq!(
            log.get(BuildId::new(1)).await,
            ["foo".into(), "bar".into()]
        );
        assert_eq!(
            log.get(BuildId::new(2)).await,
            ["bar".into(), "baz".into()]
        );
    }

    #[tokio::test]
    async fn set_duplicate_header_inputs() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into(), "foo".into()])
            .await
            .unwrap();

        assert_eq!(
            log.get(BuildId::new(1)).await,
            ["foo".into(), "foo".into()]
        );
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
        let log = HeaderInputLog::new(&directory.path().join(FILENAME), &path_pool)
            .await
            .unwrap();

        assert!(Arc::ptr_eq(
            &log.get(BuildId::new(1)).await[0],
            &path_pool.intern("foo")
        ));
    }

    #[tokio::test]
    async fn fail_to_set_header_input_with_column_separator() {
        let (log, directory) = open().await;

        assert!(
            log.set(BuildId::new(1), &["foo".into(), "bar\0baz".into()])
                .await
                .is_err()
        );
        assert_eq!(log.get(BuildId::new(1)).await, Vec::<Arc<str>>::new());
        assert_eq!(read_log(&directory), "");
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
        assert_eq!(read_log(&directory), "");
    }

    #[tokio::test]
    async fn fail_to_set_header_inputs_after_failed_write() {
        let (log, directory) = open().await;

        log.state.lock().await.file = File::open(directory.path().join(FILENAME)).unwrap();

        assert!(log.set(BuildId::new(1), &["foo".into()]).await.is_err());

        log.state.lock().await.file = open_file(&directory.path().join(FILENAME))
            .await
            .unwrap();

        assert!(log.set(BuildId::new(2), &["bar".into()]).await.is_err());
        assert_eq!(log.get(BuildId::new(1)).await, Vec::<Arc<str>>::new());
        assert_eq!(log.get(BuildId::new(2)).await, Vec::<Arc<str>>::new());
        assert_eq!(read_log(&directory), "");
    }

    mod log {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn write_lines() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            assert_eq!(
                read_log(&directory),
                format!("foo\nbar\n1{NUL}0{NUL}1\n")
            );
        }

        #[tokio::test]
        async fn write_lines_in_utf8() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["😄".into()]).await.unwrap();

            assert_eq!(read_log(&directory), format!("😄\n1{NUL}0\n"));
        }

        #[tokio::test]
        async fn write_id_in_hexadecimal() {
            let (log, directory) = open().await;

            log.set(BuildId::new(0x0123_4567_89ab_cdef), &["foo".into()])
                .await
                .unwrap();

            assert_eq!(
                read_log(&directory),
                format!("foo\n123456789abcdef{NUL}0\n")
            );
        }

        #[tokio::test]
        async fn write_index_in_decimal() {
            let (log, directory) = open().await;

            log.set(
                BuildId::new(1),
                &(0..11)
                    .map(|index| index.to_string().into())
                    .collect::<Vec<_>>(),
            )
            .await
            .unwrap();
            log.set(BuildId::new(2), &["10".into()]).await.unwrap();

            assert_eq!(
                read_log(&directory).lines().last(),
                Some(format!("2{NUL}10").as_str())
            );
        }

        #[tokio::test]
        async fn write_path_once() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(2), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            assert_eq!(
                read_log(&directory),
                format!("foo\n1{NUL}0\nbar\n2{NUL}0{NUL}1\n")
            );
        }

        #[tokio::test]
        async fn write_duplicate_path_once() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(read_log(&directory), format!("foo\n1{NUL}0{NUL}0\n"));
        }

        #[tokio::test]
        async fn write_updated_header_inputs() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(1), &["bar".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(
                read_log(&directory),
                format!("foo\n1{NUL}0\nbar\n1{NUL}1{NUL}0\n")
            );
        }

        #[tokio::test]
        async fn write_no_header_input() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(1), &[]).await.unwrap();

            assert_eq!(read_log(&directory), format!("foo\n1{NUL}0\n1{NUL}\n"));
        }

        #[tokio::test]
        async fn write_no_line_of_same_header_inputs() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            assert_eq!(read_log(&directory), format!("foo\n1{NUL}0\n"));
        }

        #[tokio::test]
        async fn write_no_line_of_no_header_input() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &[]).await.unwrap();

            assert_eq!(read_log(&directory), "");
        }

        #[tokio::test]
        async fn reopen_log() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into(), "bar".into()])
                .await
                .unwrap();
            log.set(BuildId::new(2), &["bar".into(), "baz".into()])
                .await
                .unwrap();

            drop(log);

            let log = reopen(&directory).await;

            assert_eq!(
                log.get(BuildId::new(1)).await,
                ["foo".into(), "bar".into()]
            );
            assert_eq!(
                log.get(BuildId::new(2)).await,
                ["bar".into(), "baz".into()]
            );
        }

        #[tokio::test]
        async fn reopen_log_with_updated_header_inputs() {
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
        async fn reopen_log_with_no_header_input() {
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
        async fn reopen_log_in_utf8() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["😄".into()]).await.unwrap();

            drop(log);

            assert_eq!(
                reopen(&directory).await.get(BuildId::new(1)).await,
                ["😄".into()]
            );
        }

        #[tokio::test]
        async fn reopen_log_with_large_id() {
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
        async fn reopen_log_with_duplicate_paths() {
            let directory = tempdir().unwrap();

            write_log(&directory, format!("foo\nfoo\n1{NUL}0{NUL}1\n"));

            let log = reopen(&directory).await;

            assert_eq!(
                log.get(BuildId::new(1)).await,
                ["foo".into(), "foo".into()]
            );

            log.set(BuildId::new(2), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            drop(log);

            assert_eq!(
                reopen(&directory).await.get(BuildId::new(2)).await,
                ["foo".into(), "bar".into()]
            );
        }

        #[tokio::test]
        async fn append_lines_after_reopen() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            drop(log);

            reopen(&directory)
                .await
                .set(BuildId::new(2), &["bar".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(
                read_log(&directory),
                format!("foo\n1{NUL}0\nbar\n2{NUL}1{NUL}0\n")
            );
        }

        #[tokio::test]
        async fn append_no_line_of_same_header_inputs_after_reopen() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            drop(log);

            reopen(&directory)
                .await
                .set(BuildId::new(1), &["foo".into()])
                .await
                .unwrap();

            assert_eq!(read_log(&directory), format!("foo\n1{NUL}0\n"));
        }

        #[tokio::test]
        async fn fail_to_open_log_in_invalid_utf8() {
            let directory = tempdir().unwrap();

            write_log(&directory, [b'f', 0xff, b'\n']);

            assert!(
                HeaderInputLog::new(&directory.path().join(FILENAME), &Default::default())
                    .await
                    .is_err()
            );
        }

        #[tokio::test]
        async fn fail_to_open_log_with_invalid_id() {
            let directory = tempdir().unwrap();

            write_log(&directory, format!("foo\nbar{NUL}0\n"));

            assert!(
                HeaderInputLog::new(&directory.path().join(FILENAME), &Default::default())
                    .await
                    .is_err()
            );
        }

        #[tokio::test]
        async fn fail_to_open_log_with_invalid_index() {
            let directory = tempdir().unwrap();

            write_log(&directory, format!("foo\n1{NUL}bar\n"));

            assert!(
                HeaderInputLog::new(&directory.path().join(FILENAME), &Default::default())
                    .await
                    .is_err()
            );
        }

        #[tokio::test]
        async fn fail_to_open_log_with_index_out_of_range() {
            let directory = tempdir().unwrap();

            write_log(&directory, format!("foo\n1{NUL}1\n"));

            assert!(
                HeaderInputLog::new(&directory.path().join(FILENAME), &Default::default())
                    .await
                    .is_err()
            );
        }

        #[tokio::test]
        async fn fail_to_open_log_in_missing_directory() {
            let directory = tempdir().unwrap();

            assert!(
                HeaderInputLog::new(
                    &directory.path().join("foo").join(FILENAME),
                    &Default::default()
                )
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

            write_log(
                &directory,
                format!("foo\n{}", format!("1{NUL}0\n").repeat(6)),
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(read_log(&directory), format!("foo\n1{NUL}0\n"));
        }

        #[tokio::test]
        async fn compact_updated_header_inputs() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                format!(
                    "foo\nbar\n{}1{NUL}1{NUL}0\n",
                    format!("1{NUL}0{NUL}1\n").repeat(6)
                ),
            );

            let log = reopen(&directory).await;

            assert_eq!(
                log.get(BuildId::new(1)).await,
                ["bar".into(), "foo".into()]
            );
            assert_eq!(read_log(&directory), format!("foo\nbar\n1{NUL}1{NUL}0\n"));
        }

        #[tokio::test]
        async fn compact_many_builds() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                format!(
                    "foo\nbar\n{}",
                    format!("1{NUL}0\n2{NUL}1{NUL}0\n").repeat(5)
                ),
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(
                log.get(BuildId::new(2)).await,
                ["bar".into(), "foo".into()]
            );
            assert_eq!(
                read_lines(&directory),
                [
                    format!("1{NUL}0\n"),
                    format!("2{NUL}1{NUL}0\n"),
                    "bar\n".into(),
                    "foo\n".into()
                ]
            );
        }

        #[tokio::test]
        async fn compact_paths_before_records() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                format!("foo\n{}bar\n1{NUL}1\n", format!("1{NUL}0\n").repeat(8)),
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["bar".into()]);
            assert_eq!(read_log(&directory), format!("foo\nbar\n1{NUL}1\n"));
        }

        #[tokio::test]
        async fn compact_after_appending_lines() {
            let (log, directory) = open().await;

            for inputs in [["foo"], ["bar"]].into_iter().cycle().take(8) {
                log.set(BuildId::new(1), &inputs.map(From::from))
                    .await
                    .unwrap();
            }

            drop(log);

            assert_eq!(
                read_log(&directory),
                format!(
                    "foo\n1{NUL}0\nbar\n1{NUL}1\n{}",
                    format!("1{NUL}0\n1{NUL}1\n").repeat(3)
                )
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["bar".into()]);
            assert_eq!(read_log(&directory), format!("foo\nbar\n1{NUL}1\n"));
        }

        #[tokio::test]
        async fn append_lines_after_compaction() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                format!("foo\nbar\n{}", format!("1{NUL}1\n").repeat(10)),
            );

            reopen(&directory)
                .await
                .set(BuildId::new(2), &["baz".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(
                read_log(&directory),
                format!("foo\nbar\n1{NUL}1\nbaz\n2{NUL}2{NUL}0\n")
            );
        }

        #[tokio::test]
        async fn keep_unused_path() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                format!("foo\nbar\n{}", format!("1{NUL}1\n").repeat(10)),
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["bar".into()]);
            assert_eq!(read_log(&directory), format!("foo\nbar\n1{NUL}1\n"));
        }

        #[tokio::test]
        async fn keep_log_of_optimal_size() {
            let directory = tempdir().unwrap();
            let lines = format!("foo\n1{NUL}0\nbar\n2{NUL}1{NUL}0\n");

            write_log(&directory, &lines);

            reopen(&directory).await;

            assert_eq!(read_log(&directory), lines);
        }

        #[tokio::test]
        async fn keep_log_of_compaction_ratio() {
            let directory = tempdir().unwrap();
            let lines = format!("foo\n{}", format!("1{NUL}0\n").repeat(5));

            write_log(&directory, &lines);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(read_log(&directory), lines);
        }

        #[tokio::test]
        async fn keep_empty_log() {
            let directory = tempdir().unwrap();

            write_log(&directory, "");

            reopen(&directory).await;

            assert_eq!(read_log(&directory), "");
        }
    }

    mod incomplete_line {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn remove_incomplete_path() {
            let directory = tempdir().unwrap();

            write_log(&directory, format!("foo\n1{NUL}0\nba"));

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(read_log(&directory), format!("foo\n1{NUL}0\n"));
        }

        #[tokio::test]
        async fn remove_incomplete_record() {
            let directory = tempdir().unwrap();

            write_log(&directory, format!("foo\n1{NUL}0\nbar\n2{NUL}1{NUL}"));

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)).await, Vec::<Arc<str>>::new());
            assert_eq!(read_log(&directory), format!("foo\nbar\n1{NUL}0\n"));
        }

        #[tokio::test]
        async fn remove_incomplete_record_with_invalid_index() {
            let directory = tempdir().unwrap();

            write_log(&directory, format!("foo\n1{NUL}0\n2{NUL}1"));

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)).await, Vec::<Arc<str>>::new());
            assert_eq!(read_log(&directory), format!("foo\n1{NUL}0\n"));
        }

        #[tokio::test]
        async fn remove_only_incomplete_line() {
            let directory = tempdir().unwrap();

            write_log(&directory, "fo");

            reopen(&directory).await;

            assert_eq!(read_log(&directory), "");
        }

        #[tokio::test]
        async fn remove_incomplete_line_in_incomplete_utf8() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                [
                    format!("foo\n1{NUL}0\n").as_bytes(),
                    &"😄".as_bytes()[..2],
                ]
                .concat(),
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(read_log(&directory), format!("foo\n1{NUL}0\n"));
        }

        #[tokio::test]
        async fn append_lines_after_incomplete_line() {
            let directory = tempdir().unwrap();

            write_log(&directory, format!("foo\n1{NUL}0\nba"));

            reopen(&directory)
                .await
                .set(BuildId::new(2), &["bar".into(), "foo".into()])
                .await
                .unwrap();

            assert_eq!(
                read_log(&directory),
                format!("foo\n1{NUL}0\nbar\n2{NUL}1{NUL}0\n")
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)).await, ["foo".into()]);
            assert_eq!(
                log.get(BuildId::new(2)).await,
                ["bar".into(), "foo".into()]
            );
        }
    }
}
