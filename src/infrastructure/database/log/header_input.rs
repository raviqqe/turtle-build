mod index;
mod path;

use self::{
    index::IndexLog,
    path::{PathLog, deserialize_paths},
};
use super::utility::{LINE_TERMINATOR, read_file};
use crate::{infrastructure::DatabaseError, ir::BuildId, path_pool::PathPool};
use alloc::sync::Arc;
use std::path::Path;
use tokio::{fs::create_dir_all, sync::Mutex, try_join};

const PATH_FILENAME: &str = "paths";
const INDEX_FILENAME: &str = "indices";

pub struct HeaderInputLog {
    path_log: Mutex<PathLog>,
    index_log: IndexLog,
}

impl HeaderInputLog {
    pub async fn new(directory: &Path, path_pool: &PathPool) -> Result<Self, DatabaseError> {
        create_dir_all(directory).await?;

        let path_file = directory.join(PATH_FILENAME);
        let index_file = directory.join(INDEX_FILENAME);
        let (path_bytes, index_bytes) = try_join!(read_file(&path_file), read_file(&index_file))?;
        let paths = deserialize_paths(&path_bytes, path_pool)?;
        let (path_log, index_log) = try_join!(
            PathLog::new(&path_file, &path_bytes, &paths),
            IndexLog::new(&index_file, &index_bytes, &paths)
        )?;

        Ok(Self {
            path_log: path_log.into(),
            index_log,
        })
    }

    pub fn get(&self, id: BuildId) -> Vec<Arc<str>> {
        self.index_log.get(id)
    }

    pub async fn set(&self, id: BuildId, inputs: &[Arc<str>]) -> Result<(), DatabaseError> {
        if let Some(path) = inputs
            .iter()
            .find(|path| path.contains(char::from(LINE_TERMINATOR)))
        {
            return Err(DatabaseError::new(format!(
                "invalid header input path: {path}"
            )));
        } else if self.index_log.get(id) == inputs {
            return Ok(());
        }

        let indices = self.path_log.lock().await.index(inputs)?;

        self.index_log.set(id, inputs, &indices).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{pin::pin, task::Poll};
    use futures::{future::try_join_all, poll};
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

    fn read_path_file(directory: &TempDir) -> String {
        String::from_utf8(read(directory.path().join(PATH_FILENAME)).unwrap()).unwrap()
    }

    fn read_index_file(directory: &TempDir) -> Vec<u8> {
        read(directory.path().join(INDEX_FILENAME)).unwrap()
    }

    fn write_path_file(directory: &TempDir, paths: impl AsRef<[u8]>) {
        write(directory.path().join(PATH_FILENAME), paths).unwrap();
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

        assert_eq!(log.get(BuildId::new(1)), ["foo".into(), "bar".into()]);
    }

    #[tokio::test]
    async fn get_no_header_inputs() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

        assert_eq!(log.get(BuildId::new(2)), Vec::<Arc<str>>::new());
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

        assert_eq!(log.get(BuildId::new(1)), ["bar".into(), "baz".into()]);
    }

    #[tokio::test]
    async fn update_header_inputs_to_none() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
        log.set(BuildId::new(1), &[]).await.unwrap();

        assert_eq!(log.get(BuildId::new(1)), Vec::<Arc<str>>::new());
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

        assert_eq!(log.get(BuildId::new(1)), ["foo".into(), "bar".into()]);
        assert_eq!(log.get(BuildId::new(2)), ["bar".into(), "baz".into()]);
    }

    #[tokio::test]
    async fn set_duplicate_header_inputs() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into(), "foo".into()])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)), ["foo".into(), "foo".into()]);
    }

    #[tokio::test]
    async fn set_header_input_in_directory() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo/bar baz.h".into()])
            .await
            .unwrap();

        assert_eq!(log.get(BuildId::new(1)), ["foo/bar baz.h".into()]);
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
            assert_eq!(log.get(BuildId::new(id)), inputs(id));
        }

        drop(log);

        let log = reopen(&directory).await;

        for id in 0..BUILD_COUNT {
            assert_eq!(log.get(BuildId::new(id)), inputs(id));
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
            &log.get(BuildId::new(1))[0],
            &path_pool.intern("foo")
        ));
    }

    #[tokio::test]
    async fn get_header_inputs_during_lock() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

        let _lock = log.path_log.lock().await;

        assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
    }

    #[tokio::test]
    async fn set_same_header_inputs_during_lock() {
        let (log, _directory) = open().await;

        log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

        let _lock = log.path_log.lock().await;

        assert_eq!(
            poll!(pin!(log.set(BuildId::new(1), &["foo".into()]).await)),
            Poll::Ready(Ok(()))
        );
    }

    #[tokio::test]
    async fn wait_for_lock_to_set_header_inputs() {
        let (log, _directory) = open().await;
        let lock = log.path_log.lock().await;
        let inputs = ["foo".into()];
        let mut future = pin!(log.set(BuildId::new(1), &inputs).await);

        assert!(poll!(&mut future).is_pending());

        drop(lock);

        assert_eq!(future.await, Ok(()));
        assert_eq!(log.get(BuildId::new(1)), inputs);
    }

    #[tokio::test]
    async fn fail_to_set_header_input_with_line_terminator() {
        let (log, directory) = open().await;

        assert!(
            log.set(BuildId::new(1), &["foo".into(), "bar\nbaz".into()])
                .await
                .is_err()
        );
        assert_eq!(log.get(BuildId::new(1)), Vec::<Arc<str>>::new());
        assert_eq!(read_path_file(&directory), "");
        assert_eq!(read_index_file(&directory), b"");
    }

    mod log {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn write_paths_and_record() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            assert_eq!(read_path_file(&directory), "foo\nbar\n");
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
        async fn write_path_once() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(2), &["foo".into(), "bar".into()])
                .await
                .unwrap();

            assert_eq!(read_path_file(&directory), "foo\nbar\n");
        }

        #[tokio::test]
        async fn write_nothing_for_same_header_inputs() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            let bytes = read_index_file(&directory);

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            assert_eq!(read_path_file(&directory), "foo\n");
            assert_eq!(read_index_file(&directory), bytes);
        }

        #[tokio::test]
        async fn write_nothing_for_no_header_input() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &[]).await.unwrap();

            assert_eq!(read_path_file(&directory), "");
            assert_eq!(read_index_file(&directory), b"");
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

            assert_eq!(log.get(BuildId::new(1)), ["foo".into(), "bar".into()]);
            assert_eq!(log.get(BuildId::new(2)), ["bar".into(), "baz".into()]);
        }

        #[tokio::test]
        async fn reopen_log_with_updated_header_inputs() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(1), &["bar".into()]).await.unwrap();

            drop(log);

            assert_eq!(
                reopen(&directory).await.get(BuildId::new(1)),
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
                reopen(&directory).await.get(BuildId::new(1)),
                Vec::<Arc<str>>::new()
            );
        }

        #[tokio::test]
        async fn reopen_log_in_utf8() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["😄".into()]).await.unwrap();

            drop(log);

            assert_eq!(reopen(&directory).await.get(BuildId::new(1)), ["😄".into()]);
        }

        #[tokio::test]
        async fn set_header_inputs_after_reopen() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            drop(log);

            reopen(&directory)
                .await
                .set(BuildId::new(2), &["bar".into(), "foo".into()])
                .await
                .unwrap();

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)), ["bar".into(), "foo".into()]);
            assert_eq!(read_path_file(&directory), "foo\nbar\n");
        }

        #[tokio::test]
        async fn set_same_header_inputs_after_reopen() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            drop(log);

            let bytes = read_index_file(&directory);

            reopen(&directory)
                .await
                .set(BuildId::new(1), &["foo".into()])
                .await
                .unwrap();

            assert_eq!(read_path_file(&directory), "foo\n");
            assert_eq!(read_index_file(&directory), bytes);
        }

        #[tokio::test]
        async fn set_header_inputs_after_incomplete_path() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();

            drop(log);

            write_path_file(&directory, "foo\nba");

            reopen(&directory)
                .await
                .set(BuildId::new(2), &["bar".into(), "foo".into()])
                .await
                .unwrap();

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)), ["bar".into(), "foo".into()]);
            assert_eq!(read_path_file(&directory), "foo\nbar\n");
        }

        #[tokio::test]
        async fn set_header_inputs_after_missing_path() {
            let (log, directory) = open().await;

            log.set(BuildId::new(1), &["foo".into()]).await.unwrap();
            log.set(BuildId::new(2), &["bar".into()]).await.unwrap();

            drop(log);

            write_path_file(&directory, "foo\n");

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)), Vec::<Arc<str>>::new());

            log.set(BuildId::new(3), &["baz".into()]).await.unwrap();

            drop(log);

            let log = reopen(&directory).await;

            assert_eq!(log.get(BuildId::new(1)), ["foo".into()]);
            assert_eq!(log.get(BuildId::new(2)), Vec::<Arc<str>>::new());
            assert_eq!(log.get(BuildId::new(3)), ["baz".into()]);
            assert_eq!(read_path_file(&directory), "foo\nbaz\n");
        }

        #[tokio::test]
        async fn fail_to_open_log_with_path_in_invalid_utf8() {
            let directory = tempdir().unwrap();

            write_path_file(&directory, [b'f', 0xff, b'\n']);

            assert!(
                HeaderInputLog::new(directory.path(), &Default::default())
                    .await
                    .is_err()
            );
        }
    }
}
