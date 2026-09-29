use super::utility::{
    COLUMN_SEPARATOR, COMPACTION_RATIO, LINE_TERMINATOR, compact_file, open_file, read_file,
    split_lines,
};
use crate::infrastructure::DatabaseError;
use core::str;
use scc::{Guard, HashIndex, hash_index::Entry};
use std::{fs::File, io::Write, path::Path};

pub struct SourceLog {
    file: File,
    sources: HashIndex<String, String>,
}

impl SourceLog {
    // TODO Load a file lazily.
    pub async fn new(path: &Path) -> Result<Self, DatabaseError> {
        let bytes = read_file(path).await?;
        let lines = split_lines(&bytes).collect::<Vec<_>>();
        let sources = HashIndex::<String, String>::with_capacity(lines.len());

        for line in lines.into_iter().rev() {
            let (output, source) = str::from_utf8(line)?
                .split_once(char::from(COLUMN_SEPARATOR))
                .ok_or_else(|| DatabaseError::new("column separator not found in source log"))?;

            sources.insert_sync(output.into(), source.into()).ok();
        }

        if bytes.last().is_some_and(|&byte| byte != LINE_TERMINATOR)
            || bytes.len()
                > COMPACTION_RATIO
                    * sources
                        .iter(&Guard::new())
                        .map(|(output, source)| serialize(output, source).len())
                        .sum::<usize>()
        {
            // Do not inline this to avoid holding a guard across an await point.
            let bytes = sources
                .iter(&Guard::new())
                .flat_map(|(output, source)| serialize(output, source))
                .collect::<Vec<_>>();

            compact_file(path, bytes).await?;
        }

        Ok(Self {
            file: open_file(path).await?,
            sources,
        })
    }

    pub fn get(&self, output: &str) -> Option<String> {
        self.sources.peek_with(output, |_, source| source.clone())
    }

    pub fn set(&self, output: &str, source: &str) -> Result<(), DatabaseError> {
        if self
            .sources
            .peek_with(output, |_, value| value != source)
            .unwrap_or(true)
        {
            (&self.file).write_all(&serialize(output, source))?;

            match self.sources.entry_sync(output.into()) {
                Entry::Occupied(mut entry) => entry.update(source.into()),
                Entry::Vacant(entry) => {
                    entry.insert_entry(source.into());
                }
            }
        }

        Ok(())
    }
}

fn serialize(output: &str, source: &str) -> Vec<u8> {
    [
        output.as_bytes(),
        &[COLUMN_SEPARATOR],
        source.as_bytes(),
        &[LINE_TERMINATOR],
    ]
    .concat()
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

    async fn open() -> (SourceLog, TempDir) {
        let directory = tempdir().unwrap();

        (reopen(&directory).await, directory)
    }

    async fn reopen(directory: &TempDir) -> SourceLog {
        SourceLog::new(&directory.path().join(FILENAME))
            .await
            .unwrap()
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
    async fn source() {
        let (log, _directory) = open().await;

        log.set("foo", "bar").unwrap();

        assert_eq!(log.get("foo"), Some("bar".into()));
    }

    #[tokio::test]
    async fn get_no_source() {
        let (log, _directory) = open().await;

        log.set("foo", "bar").unwrap();

        assert_eq!(log.get("baz"), None);
    }

    #[tokio::test]
    async fn update_source() {
        let (log, _directory) = open().await;

        log.set("foo", "bar").unwrap();
        log.set("foo", "baz").unwrap();

        assert_eq!(log.get("foo"), Some("baz".into()));
    }

    #[tokio::test]
    async fn set_sources() {
        let (log, _directory) = open().await;

        log.set("foo", "bar").unwrap();
        log.set("baz", "qux").unwrap();

        assert_eq!(log.get("foo"), Some("bar".into()));
        assert_eq!(log.get("baz"), Some("qux".into()));
    }

    #[tokio::test]
    async fn set_same_source_of_outputs() {
        let (log, _directory) = open().await;

        log.set("foo", "baz").unwrap();
        log.set("bar", "baz").unwrap();

        assert_eq!(log.get("foo"), Some("baz".into()));
        assert_eq!(log.get("bar"), Some("baz".into()));
    }

    #[tokio::test]
    async fn set_source_in_directory() {
        let (log, _directory) = open().await;

        log.set("foo/bar baz.o", "foo/bar baz.c").unwrap();

        assert_eq!(log.get("foo/bar baz.o"), Some("foo/bar baz.c".into()));
    }

    #[tokio::test]
    async fn set_empty_source() {
        let (log, _directory) = open().await;

        log.set("foo", "").unwrap();

        assert_eq!(log.get("foo"), Some("".into()));
    }

    #[tokio::test]
    async fn set_sources_concurrently() {
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
                        log.set(output, &format!("{output}.c")).unwrap();
                    }
                });
            }
        });

        drop(log);

        let log = reopen(&directory).await;

        for output in outputs {
            assert_eq!(log.get(&output), Some(format!("{output}.c")));
        }
    }

    #[tokio::test]
    async fn write_line() {
        let (log, directory) = open().await;

        log.set("foo", "bar").unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\n");
    }

    #[tokio::test]
    async fn write_line_in_utf8() {
        let (log, directory) = open().await;

        log.set("😄", "🚀").unwrap();

        assert_eq!(
            read_log(&directory),
            [0xf0, 0x9f, 0x98, 0x84, 0, 0xf0, 0x9f, 0x9a, 0x80, b'\n']
        );
    }

    #[tokio::test]
    async fn append_lines() {
        let (log, directory) = open().await;

        log.set("foo", "bar").unwrap();
        log.set("baz", "qux").unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\nbaz\0qux\n");
    }

    #[tokio::test]
    async fn append_line_of_updated_source() {
        let (log, directory) = open().await;

        log.set("foo", "bar").unwrap();
        log.set("foo", "baz").unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\nfoo\0baz\n");
    }

    #[tokio::test]
    async fn append_no_line_of_same_source() {
        let (log, directory) = open().await;

        log.set("foo", "bar").unwrap();
        log.set("baz", "qux").unwrap();
        log.set("foo", "bar").unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\nbaz\0qux\n");
    }

    #[tokio::test]
    async fn reopen_log() {
        let (log, directory) = open().await;

        log.set("foo", "bar").unwrap();
        log.set("baz", "qux").unwrap();

        drop(log);

        let log = reopen(&directory).await;

        assert_eq!(log.get("foo"), Some("bar".into()));
        assert_eq!(log.get("baz"), Some("qux".into()));
    }

    #[tokio::test]
    async fn reopen_log_with_updated_source() {
        let (log, directory) = open().await;

        log.set("foo", "bar").unwrap();
        log.set("foo", "baz").unwrap();

        drop(log);

        assert_eq!(reopen(&directory).await.get("foo"), Some("baz".into()));
    }

    #[tokio::test]
    async fn reopen_log_in_utf8() {
        let (log, directory) = open().await;

        log.set("😄", "🚀").unwrap();

        drop(log);

        assert_eq!(reopen(&directory).await.get("😄"), Some("🚀".into()));
    }

    #[tokio::test]
    async fn reopen_log_with_empty_source() {
        let (log, directory) = open().await;

        log.set("foo", "").unwrap();

        drop(log);

        assert_eq!(reopen(&directory).await.get("foo"), Some("".into()));
    }

    #[tokio::test]
    async fn reopen_log_with_column_separator_in_source() {
        let directory = tempdir().unwrap();

        write_log(&directory, "foo\0bar\0baz\n");

        assert_eq!(reopen(&directory).await.get("foo"), Some("bar\0baz".into()));
    }

    #[tokio::test]
    async fn append_lines_after_reopen() {
        let (log, directory) = open().await;

        log.set("foo", "bar").unwrap();

        drop(log);

        reopen(&directory).await.set("baz", "qux").unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\nbaz\0qux\n");
    }

    #[tokio::test]
    async fn append_line_of_updated_source_after_reopen() {
        let (log, directory) = open().await;

        log.set("foo", "bar").unwrap();

        drop(log);

        reopen(&directory).await.set("foo", "baz").unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\nfoo\0baz\n");
    }

    #[tokio::test]
    async fn append_no_line_of_same_source_after_reopen() {
        let (log, directory) = open().await;

        log.set("foo", "bar").unwrap();

        drop(log);

        reopen(&directory).await.set("foo", "bar").unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\n");
    }

    #[tokio::test]
    async fn fail_to_open_log_in_invalid_utf8() {
        let directory = tempdir().unwrap();

        write_log(&directory, [b'f', 0, 0xff, b'\n']);

        assert!(
            SourceLog::new(&directory.path().join(FILENAME))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn fail_to_open_log_without_column_separator() {
        let directory = tempdir().unwrap();

        write_log(&directory, "foo\0bar\nbaz\n");

        assert!(
            SourceLog::new(&directory.path().join(FILENAME))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn fail_to_open_log_in_missing_directory() {
        let directory = tempdir().unwrap();

        assert!(
            SourceLog::new(&directory.path().join("foo").join(FILENAME))
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

            write_log(&directory, "foo\0bar\nfoo\0bar\nfoo\0bar\nfoo\0baz\n");

            let log = reopen(&directory).await;

            assert_eq!(log.get("foo"), Some("baz".into()));
            assert_eq!(read_log(&directory), b"foo\0baz\n");
        }

        #[tokio::test]
        async fn compact_many_outputs() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                "foo\0bar\nbaz\0qux\nfoo\0bar\nbaz\0qux\nfoo\0bar\nbaz\0qux\nfoo\0bar\n",
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get("foo"), Some("bar".into()));
            assert_eq!(log.get("baz"), Some("qux".into()));
            assert_eq!(read_lines(&directory), ["baz\0qux\n", "foo\0bar\n"]);
        }

        #[tokio::test]
        async fn compact_sources_of_different_lengths() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                "foo\0bar\nbaz\0foo/bar\nbaz\0foo/bar\nbaz\0foo/bar\nbaz\0foo/bar\nbaz\0foo/bar\n",
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get("foo"), Some("bar".into()));
            assert_eq!(log.get("baz"), Some("foo/bar".into()));
            assert_eq!(read_lines(&directory), ["baz\0foo/bar\n", "foo\0bar\n"]);
        }

        #[tokio::test]
        async fn compact_after_appending_lines() {
            let (log, directory) = open().await;

            for source in ["bar", "baz", "bar", "baz"] {
                log.set("foo", source).unwrap();
            }

            drop(log);

            assert_eq!(
                read_log(&directory),
                b"foo\0bar\nfoo\0baz\nfoo\0bar\nfoo\0baz\n"
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get("foo"), Some("baz".into()));
            assert_eq!(read_log(&directory), b"foo\0baz\n");
        }

        #[tokio::test]
        async fn append_line_after_compaction() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\0bar\nfoo\0bar\nfoo\0bar\nfoo\0bar\n");

            reopen(&directory).await.set("baz", "qux").unwrap();

            assert_eq!(read_log(&directory), b"foo\0bar\nbaz\0qux\n");
        }

        #[tokio::test]
        async fn keep_log_of_optimal_size() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\0bar\nbaz\0qux\n");

            reopen(&directory).await;

            assert_eq!(read_log(&directory), b"foo\0bar\nbaz\0qux\n");
        }

        #[tokio::test]
        async fn keep_log_of_compaction_ratio() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\0bar\nfoo\0bar\nfoo\0baz\n");

            let log = reopen(&directory).await;

            assert_eq!(log.get("foo"), Some("baz".into()));
            assert_eq!(read_log(&directory), b"foo\0bar\nfoo\0bar\nfoo\0baz\n");
        }

        #[tokio::test]
        async fn keep_log_of_compaction_ratio_with_sources_of_different_lengths() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                "foo\0bar\nfoo\0bar\nfoo\0bar\nfoo\0bar\nfoo\0bar\nfoo\0bar\nbaz\0foo/bar\n",
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get("foo"), Some("bar".into()));
            assert_eq!(log.get("baz"), Some("foo/bar".into()));
            assert_eq!(
                read_log(&directory),
                b"foo\0bar\nfoo\0bar\nfoo\0bar\nfoo\0bar\nfoo\0bar\nfoo\0bar\nbaz\0foo/bar\n"
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

            write_log(&directory, "foo\0bar\nbaz\0qu");

            let log = reopen(&directory).await;

            assert_eq!(log.get("foo"), Some("bar".into()));
            assert_eq!(log.get("baz"), None);
            assert_eq!(read_log(&directory), b"foo\0bar\n");
        }

        #[tokio::test]
        async fn remove_incomplete_line_without_column_separator() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\0bar\nba");

            let log = reopen(&directory).await;

            assert_eq!(log.get("foo"), Some("bar".into()));
            assert_eq!(read_log(&directory), b"foo\0bar\n");
        }

        #[tokio::test]
        async fn remove_only_incomplete_line() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\0ba");

            let log = reopen(&directory).await;

            assert_eq!(log.get("foo"), None);
            assert_eq!(read_log(&directory), b"");
        }

        #[tokio::test]
        async fn remove_incomplete_line_in_incomplete_utf8() {
            let directory = tempdir().unwrap();

            write_log(
                &directory,
                [b"foo\0bar\n".as_slice(), &"😄".as_bytes()[..2]].concat(),
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get("foo"), Some("bar".into()));
            assert_eq!(read_log(&directory), b"foo\0bar\n");
        }

        #[tokio::test]
        async fn append_line_after_incomplete_line() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\0bar\nbaz\0qu");

            reopen(&directory).await.set("baz", "qux").unwrap();

            assert_eq!(read_log(&directory), b"foo\0bar\nbaz\0qux\n");

            let log = reopen(&directory).await;

            assert_eq!(log.get("foo"), Some("bar".into()));
            assert_eq!(log.get("baz"), Some("qux".into()));
        }
    }
}
