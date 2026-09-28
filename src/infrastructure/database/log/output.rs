use super::utility::{COMPACTION_RATIO, compact_file, open_file, read_file};
use crate::infrastructure::DatabaseError;
use core::str;
use scc::{Guard, HashIndex};
use std::{fs::File, io::Write, path::Path};

const LINE_TERMINATOR: u8 = b'\n';

pub struct OutputLog {
    file: File,
    outputs: HashIndex<String, ()>,
}

impl OutputLog {
    pub async fn new(path: &Path) -> Result<Self, DatabaseError> {
        let bytes = read_file(path).await?;
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
                .flat_map(|(path, _)| serialize(path))
                .collect::<Vec<_>>();

            compact_file(path, bytes).await?;
        }

        Ok(Self {
            file: open_file(path).await?,
            outputs,
        })
    }

    pub fn get(&self) -> Vec<String> {
        self.outputs
            .iter(&Guard::new())
            .map(|(path, _)| path.clone())
            .collect()
    }

    pub fn set(&self, path: &str) -> Result<(), DatabaseError> {
        if self.outputs.insert_sync(path.into(), ()).is_ok() {
            (&self.file).write_all(&serialize(path))?;
        }

        Ok(())
    }
}

fn serialize(path: &str) -> Vec<u8> {
    [path.as_bytes(), &[LINE_TERMINATOR]].concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::{
        fs::{exists, read, write},
        thread::scope,
    };
    use tempfile::{TempDir, tempdir};

    const FILENAME: &str = "log";

    async fn open() -> (OutputLog, TempDir) {
        let directory = tempdir().unwrap();

        (reopen(&directory).await, directory)
    }

    async fn reopen(directory: &TempDir) -> OutputLog {
        OutputLog::new(&directory.path().join(FILENAME))
            .await
            .unwrap()
    }

    fn get_outputs(log: &OutputLog) -> Vec<String> {
        let mut outputs = log.get();

        outputs.sort();

        outputs
    }

    fn read_log(directory: &TempDir) -> Vec<u8> {
        read(directory.path().join(FILENAME)).unwrap()
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
        write(directory.path().join(FILENAME), log).unwrap();
    }

    #[tokio::test]
    async fn new() {
        let (_log, directory) = open().await;

        assert!(exists(directory.path().join(FILENAME)).unwrap());
    }

    #[tokio::test]
    async fn set_output() {
        let (log, _directory) = open().await;

        log.set("foo").unwrap();

        assert_eq!(log.get(), ["foo"]);
    }

    #[tokio::test]
    async fn get_no_output() {
        let (log, _directory) = open().await;

        assert_eq!(log.get(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn set_outputs() {
        let (log, _directory) = open().await;

        log.set("foo").unwrap();
        log.set("bar").unwrap();

        assert_eq!(get_outputs(&log), ["bar", "foo"]);
    }

    #[tokio::test]
    async fn set_output_twice() {
        let (log, _directory) = open().await;

        log.set("foo").unwrap();
        log.set("foo").unwrap();

        assert_eq!(log.get(), ["foo"]);
    }

    #[tokio::test]
    async fn set_output_in_directory() {
        let (log, _directory) = open().await;

        log.set("foo/bar baz.o").unwrap();

        assert_eq!(log.get(), ["foo/bar baz.o"]);
    }

    #[tokio::test]
    async fn set_outputs_concurrently() {
        const THREAD_COUNT: usize = 8;
        const OUTPUT_COUNT: usize = 256;

        let (log, directory) = open().await;
        let outputs = (0..THREAD_COUNT * OUTPUT_COUNT)
            .map(|index| format!("{index:04}"))
            .collect::<Vec<_>>();

        scope(|scope| {
            for outputs in outputs.chunks(OUTPUT_COUNT) {
                let log = &log;

                scope.spawn(move || {
                    for output in outputs {
                        log.set(output).unwrap();
                    }
                });
            }
        });

        assert_eq!(get_outputs(&log), outputs);

        drop(log);

        assert_eq!(get_outputs(&reopen(&directory).await), outputs);
    }

    #[tokio::test]
    async fn write_line() {
        let (log, directory) = open().await;

        log.set("foo").unwrap();

        assert_eq!(read_log(&directory), b"foo\n");
    }

    #[tokio::test]
    async fn write_line_in_utf8() {
        let (log, directory) = open().await;

        log.set("😄").unwrap();

        assert_eq!(read_log(&directory), [0xf0, 0x9f, 0x98, 0x84, b'\n']);
    }

    #[tokio::test]
    async fn append_lines() {
        let (log, directory) = open().await;

        log.set("foo").unwrap();
        log.set("bar").unwrap();

        assert_eq!(read_log(&directory), b"foo\nbar\n");
    }

    #[tokio::test]
    async fn append_no_line_of_same_output() {
        let (log, directory) = open().await;

        log.set("foo").unwrap();
        log.set("bar").unwrap();
        log.set("foo").unwrap();

        assert_eq!(read_log(&directory), b"foo\nbar\n");
    }

    #[tokio::test]
    async fn reopen_log() {
        let (log, directory) = open().await;

        log.set("foo").unwrap();
        log.set("bar").unwrap();

        drop(log);

        assert_eq!(get_outputs(&reopen(&directory).await), ["bar", "foo"]);
    }

    #[tokio::test]
    async fn reopen_log_in_utf8() {
        let (log, directory) = open().await;

        log.set("😄").unwrap();

        drop(log);

        assert_eq!(reopen(&directory).await.get(), ["😄"]);
    }

    #[tokio::test]
    async fn reopen_log_with_empty_line() {
        let directory = tempdir().unwrap();

        write_log(&directory, "foo\n\nbar\n");

        assert_eq!(get_outputs(&reopen(&directory).await), ["", "bar", "foo"]);
    }

    #[tokio::test]
    async fn append_lines_after_reopen() {
        let (log, directory) = open().await;

        log.set("foo").unwrap();

        drop(log);

        reopen(&directory).await.set("bar").unwrap();

        assert_eq!(read_log(&directory), b"foo\nbar\n");
    }

    #[tokio::test]
    async fn append_no_line_of_same_output_after_reopen() {
        let (log, directory) = open().await;

        log.set("foo").unwrap();

        drop(log);

        reopen(&directory).await.set("foo").unwrap();

        assert_eq!(read_log(&directory), b"foo\n");
    }

    #[tokio::test]
    async fn fail_to_open_log_in_invalid_utf8() {
        let directory = tempdir().unwrap();

        write_log(&directory, [b'f', 0xff, b'\n']);

        assert!(
            OutputLog::new(&directory.path().join(FILENAME))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn fail_to_open_log_in_missing_directory() {
        let directory = tempdir().unwrap();

        assert!(
            OutputLog::new(&directory.path().join("foo").join(FILENAME))
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

            write_log(&directory, "foo\nfoo\nfoo\nfoo\n");

            let log = reopen(&directory).await;

            assert_eq!(log.get(), ["foo"]);
            assert_eq!(read_log(&directory), b"foo\n");
        }

        #[tokio::test]
        async fn compact_many_outputs() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\nbar\nfoo\nbar\nfoo\nbar\nfoo\n");

            let log = reopen(&directory).await;

            assert_eq!(get_outputs(&log), ["bar", "foo"]);
            assert_eq!(read_lines(&directory), ["bar\n", "foo\n"]);
        }

        #[tokio::test]
        async fn compact_outputs_of_different_lengths() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                "foo\nfoo/bar\nfoo/bar\nfoo/bar\nfoo/bar\nfoo/bar\n",
            );

            let log = reopen(&directory).await;

            assert_eq!(get_outputs(&log), ["foo", "foo/bar"]);
            assert_eq!(read_lines(&directory), ["foo\n", "foo/bar\n"]);
        }

        #[tokio::test]
        async fn append_line_after_compaction() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\nfoo\nfoo\nfoo\n");

            reopen(&directory).await.set("bar").unwrap();

            assert_eq!(read_log(&directory), b"foo\nbar\n");
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

            let log = reopen(&directory).await;

            assert_eq!(log.get(), ["foo"]);
            assert_eq!(read_log(&directory), b"foo\nfoo\nfoo\n");
        }

        #[tokio::test]
        async fn keep_log_of_compaction_ratio_with_outputs_of_different_lengths() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\nfoo\nfoo\nfoo\nfoo\nfoo\nfoo/bar\n");

            let log = reopen(&directory).await;

            assert_eq!(get_outputs(&log), ["foo", "foo/bar"]);
            assert_eq!(
                read_log(&directory),
                b"foo\nfoo\nfoo\nfoo\nfoo\nfoo\nfoo/bar\n"
            );
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

            let log = reopen(&directory).await;

            assert_eq!(log.get(), ["foo"]);
            assert_eq!(read_log(&directory), b"foo\n");
        }

        #[tokio::test]
        async fn remove_only_incomplete_line() {
            let directory = tempdir().unwrap();

            write_log(&directory, "fo");

            let log = reopen(&directory).await;

            assert_eq!(log.get(), Vec::<String>::new());
            assert_eq!(read_log(&directory), b"");
        }

        #[tokio::test]
        async fn remove_incomplete_line_in_incomplete_utf8() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                [b"foo\n".as_slice(), &"😄".as_bytes()[..2]].concat(),
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get(), ["foo"]);
            assert_eq!(read_log(&directory), b"foo\n");
        }

        #[tokio::test]
        async fn append_line_after_incomplete_line() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\nba");

            reopen(&directory).await.set("bar").unwrap();

            assert_eq!(read_log(&directory), b"foo\nbar\n");
            assert_eq!(get_outputs(&reopen(&directory).await), ["bar", "foo"]);
        }
    }
}
