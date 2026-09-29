use super::super::utility::{LINE_TERMINATOR, open_log, split_lines};
use crate::{infrastructure::DatabaseError, path_pool::PathPool};
use alloc::sync::Arc;
use core::str;
use std::{collections::HashMap, fs::File, io::Write, path::Path};

pub struct PathLog {
    file: File,
    count: u32,
    indices: HashMap<Arc<str>, u32>,
    failed: bool,
}

impl PathLog {
    pub async fn new(path: &Path, bytes: &[u8], paths: &[Arc<str>]) -> Result<Self, DatabaseError> {
        Ok(Self {
            file: open_log(
                path,
                bytes
                    .last()
                    .is_some_and(|&byte| byte != LINE_TERMINATOR)
                    .then(|| serialize_paths(paths)),
            )
            .await?,
            count: paths.len().try_into()?,
            indices: paths.iter().cloned().zip(0..).collect(),
            failed: false,
        })
    }

    // This needs a lock to keep paths in the same order in the file and memory.
    pub fn index(&mut self, paths: &[Arc<str>]) -> Result<Vec<u32>, DatabaseError> {
        if self.failed {
            return Err(DatabaseError::new("header input log failed to be written"));
        }

        let mut new_paths = vec![];
        let indices = paths
            .iter()
            .map(|path| {
                *self.indices.entry(path.clone()).or_insert_with(|| {
                    new_paths.push(path.clone());
                    self.count += 1;
                    self.count - 1
                })
            })
            .collect();

        if let Err(error) = self.file.write_all(&serialize_paths(&new_paths)) {
            // The file and memory might have different paths.
            self.failed = true;

            return Err(error.into());
        }

        Ok(indices)
    }
}

pub fn deserialize_paths(
    bytes: &[u8],
    path_pool: &PathPool,
) -> Result<Vec<Arc<str>>, DatabaseError> {
    split_lines(bytes)
        .map(|line| Ok(path_pool.intern(str::from_utf8(line)?)))
        .collect()
}

fn serialize_paths(paths: &[Arc<str>]) -> Vec<u8> {
    paths
        .iter()
        .flat_map(|path| [path.as_bytes(), &[LINE_TERMINATOR]].concat())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{super::super::utility::open_file, *};
    use pretty_assertions::assert_eq;
    use std::fs::{exists, read, write};
    use tempfile::{TempDir, tempdir};

    const FILENAME: &str = "log";

    async fn open() -> (PathLog, TempDir) {
        let directory = tempdir().unwrap();

        (reopen(&directory).await, directory)
    }

    async fn reopen(directory: &TempDir) -> PathLog {
        let path = directory.path().join(FILENAME);
        let bytes = read(&path).unwrap_or_default();

        PathLog::new(
            &path,
            &bytes,
            &deserialize_paths(&bytes, &Default::default()).unwrap(),
        )
        .await
        .unwrap()
    }

    fn read_log(directory: &TempDir) -> String {
        String::from_utf8(read(directory.path().join(FILENAME)).unwrap()).unwrap()
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
    async fn index_path() {
        let (mut log, _directory) = open().await;

        assert_eq!(log.index(&["foo".into()]).unwrap(), [0]);
    }

    #[tokio::test]
    async fn index_paths() {
        let (mut log, _directory) = open().await;

        assert_eq!(log.index(&["foo".into(), "bar".into()]).unwrap(), [0, 1]);
    }

    #[tokio::test]
    async fn index_no_path() {
        let (mut log, _directory) = open().await;

        assert_eq!(log.index(&[]).unwrap(), [0u32; 0]);
    }

    #[tokio::test]
    async fn index_path_twice() {
        let (mut log, _directory) = open().await;

        assert_eq!(log.index(&["foo".into(), "bar".into()]).unwrap(), [0, 1]);
        assert_eq!(log.index(&["bar".into(), "baz".into()]).unwrap(), [1, 2]);
    }

    #[tokio::test]
    async fn index_duplicate_paths() {
        let (mut log, _directory) = open().await;

        assert_eq!(
            log.index(&["foo".into(), "bar".into(), "foo".into()])
                .unwrap(),
            [0, 1, 0]
        );
    }

    #[tokio::test]
    async fn write_paths() {
        let (mut log, directory) = open().await;

        log.index(&["foo".into(), "bar".into()]).unwrap();

        assert_eq!(read_log(&directory), "foo\nbar\n");
    }

    #[tokio::test]
    async fn write_path_in_directory() {
        let (mut log, directory) = open().await;

        log.index(&["foo/bar baz.h".into()]).unwrap();

        assert_eq!(read_log(&directory), "foo/bar baz.h\n");
    }

    #[tokio::test]
    async fn write_path_in_utf8() {
        let (mut log, directory) = open().await;

        log.index(&["😄".into()]).unwrap();

        assert_eq!(read_log(&directory), "😄\n");
    }

    #[tokio::test]
    async fn write_path_once() {
        let (mut log, directory) = open().await;

        log.index(&["foo".into()]).unwrap();
        log.index(&["foo".into(), "bar".into()]).unwrap();

        assert_eq!(read_log(&directory), "foo\nbar\n");
    }

    #[tokio::test]
    async fn write_duplicate_path_once() {
        let (mut log, directory) = open().await;

        log.index(&["foo".into(), "foo".into()]).unwrap();

        assert_eq!(read_log(&directory), "foo\n");
    }

    #[tokio::test]
    async fn write_no_path() {
        let (mut log, directory) = open().await;

        log.index(&[]).unwrap();

        assert_eq!(read_log(&directory), "");
    }

    #[tokio::test]
    async fn reopen_log() {
        let (mut log, directory) = open().await;

        log.index(&["foo".into(), "bar".into()]).unwrap();

        drop(log);

        assert_eq!(
            reopen(&directory)
                .await
                .index(&["bar".into(), "foo".into()])
                .unwrap(),
            [1, 0]
        );
        assert_eq!(read_log(&directory), "foo\nbar\n");
    }

    #[tokio::test]
    async fn reopen_log_with_duplicate_paths() {
        let directory = tempdir().unwrap();

        write_log(&directory, "foo\nfoo\n");

        assert_eq!(
            reopen(&directory)
                .await
                .index(&["foo".into(), "bar".into()])
                .unwrap(),
            [1, 2]
        );
        assert_eq!(read_log(&directory), "foo\nfoo\nbar\n");
    }

    #[tokio::test]
    async fn append_paths_after_reopen() {
        let (mut log, directory) = open().await;

        log.index(&["foo".into()]).unwrap();

        drop(log);

        assert_eq!(
            reopen(&directory)
                .await
                .index(&["bar".into(), "foo".into()])
                .unwrap(),
            [1, 0]
        );
        assert_eq!(read_log(&directory), "foo\nbar\n");
    }

    #[tokio::test]
    async fn fail_to_open_log_in_missing_directory() {
        let directory = tempdir().unwrap();

        assert!(
            PathLog::new(&directory.path().join("foo").join(FILENAME), &[], &[])
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn fail_to_index_paths_on_failed_write() {
        let (mut log, directory) = open().await;

        log.file = File::open(directory.path().join(FILENAME)).unwrap();

        assert!(log.index(&["foo".into()]).is_err());
        assert_eq!(read_log(&directory), "");
    }

    #[tokio::test]
    async fn fail_to_index_paths_after_failed_write() {
        let (mut log, directory) = open().await;

        log.file = File::open(directory.path().join(FILENAME)).unwrap();

        assert!(log.index(&["foo".into()]).is_err());

        log.file = open_file(&directory.path().join(FILENAME)).await.unwrap();

        assert!(log.index(&["bar".into()]).is_err());
        assert!(log.index(&[]).is_err());
        assert_eq!(read_log(&directory), "");
    }

    mod incomplete_path {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn remove_incomplete_path() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\nba");

            reopen(&directory).await;

            assert_eq!(read_log(&directory), "foo\n");
        }

        #[tokio::test]
        async fn remove_only_incomplete_path() {
            let directory = tempdir().unwrap();

            write_log(&directory, "fo");

            reopen(&directory).await;

            assert_eq!(read_log(&directory), "");
        }

        #[tokio::test]
        async fn remove_incomplete_path_in_incomplete_utf8() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                [b"foo\n".as_slice(), &"😄".as_bytes()[..2]].concat(),
            );

            reopen(&directory).await;

            assert_eq!(read_log(&directory), "foo\n");
        }

        #[tokio::test]
        async fn append_path_after_incomplete_path() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\nba");

            assert_eq!(
                reopen(&directory)
                    .await
                    .index(&["bar".into(), "foo".into()])
                    .unwrap(),
                [1, 0]
            );
            assert_eq!(read_log(&directory), "foo\nbar\n");
        }

        #[tokio::test]
        async fn keep_complete_paths() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\nbar\n");

            reopen(&directory).await;

            assert_eq!(read_log(&directory), "foo\nbar\n");
        }

        #[tokio::test]
        async fn keep_empty_log() {
            let directory = tempdir().unwrap();

            write_log(&directory, "");

            reopen(&directory).await;

            assert_eq!(read_log(&directory), "");
        }
    }

    mod deserialization {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn deserialize_no_path() {
            assert_eq!(
                deserialize_paths(b"", &Default::default()).unwrap(),
                Vec::<Arc<str>>::new()
            );
        }

        #[test]
        fn deserialize_path() {
            assert_eq!(
                deserialize_paths(b"foo\n", &Default::default()).unwrap(),
                ["foo".into()]
            );
        }

        #[test]
        fn deserialize_many_paths() {
            assert_eq!(
                deserialize_paths(b"foo\nbar\nbaz\n", &Default::default()).unwrap(),
                ["foo".into(), "bar".into(), "baz".into()]
            );
        }

        #[test]
        fn deserialize_duplicate_paths() {
            assert_eq!(
                deserialize_paths(b"foo\nfoo\n", &Default::default()).unwrap(),
                ["foo".into(), "foo".into()]
            );
        }

        #[test]
        fn deserialize_path_in_utf8() {
            assert_eq!(
                deserialize_paths("😄\n".as_bytes(), &Default::default()).unwrap(),
                ["😄".into()]
            );
        }

        #[test]
        fn deserialize_no_incomplete_path() {
            assert_eq!(
                deserialize_paths(b"foo\nba", &Default::default()).unwrap(),
                ["foo".into()]
            );
        }

        #[test]
        fn intern_path() {
            let path_pool = PathPool::new();

            assert!(Arc::ptr_eq(
                &deserialize_paths(b"foo\n", &path_pool).unwrap()[0],
                &path_pool.intern("foo")
            ));
        }

        #[test]
        fn fail_to_deserialize_path_in_invalid_utf8() {
            assert!(deserialize_paths(&[b'f', 0xff, b'\n'], &Default::default()).is_err());
        }
    }
}
