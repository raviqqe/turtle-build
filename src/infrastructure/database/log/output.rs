use super::utility::{
    COLUMN_SEPARATOR, COMPACTION_RATIO, LINE_TERMINATOR, compact_file, open_file, read_file,
    split_lines,
};
use crate::infrastructure::DatabaseError;
use core::str;
use rapidhash::fast::RandomState;
use scc::{Guard, HashIndex, hash_index::Entry};
use std::{fs::File, io::Write, path::Path};

pub struct OutputLog {
    file: File,
    outputs: HashIndex<String, Option<String>, RandomState>,
}

impl OutputLog {
    // TODO Load a file lazily.
    pub async fn new(path: &Path) -> Result<Self, DatabaseError> {
        let bytes = read_file(path)?;
        let lines = split_lines(&bytes).collect::<Vec<_>>();
        let outputs = HashIndex::<String, Option<String>, _>::with_capacity_and_hasher(
            lines.len(),
            Default::default(),
        );

        for line in lines.into_iter().rev() {
            let (output, source) = deserialize(line)?;

            outputs
                .insert_sync(output.into(), source.map(From::from))
                .ok();
        }

        if bytes.last().is_some_and(|&byte| byte != LINE_TERMINATOR)
            || bytes.len()
                > COMPACTION_RATIO
                    * outputs
                        .iter(&Guard::new())
                        .map(|(output, source)| serialize(output, source.as_deref()).len())
                        .sum::<usize>()
        {
            compact_file(
                path,
                outputs
                    .iter(&Guard::new())
                    .flat_map(|(output, source)| serialize(output, source.as_deref()))
                    .collect(),
            )?;
        }

        Ok(Self {
            file: open_file(path)?,
            outputs,
        })
    }

    pub fn get(&self) -> Vec<String> {
        self.outputs
            .iter(&Guard::new())
            .map(|(output, _)| output.clone())
            .collect()
    }

    pub fn get_source(&self, output: &str) -> Option<String> {
        self.outputs
            .peek_with(output, |_, source| source.clone())
            .flatten()
    }

    pub async fn set(&self, output: &str, source: Option<&str>) -> Result<(), DatabaseError> {
        if self
            .outputs
            .peek_with(output, |_, value| value.as_deref() != source)
            .unwrap_or(true)
        {
            (&self.file).write_all(&serialize(output, source))?;

            match self.outputs.entry_async(output.into()).await {
                Entry::Occupied(mut entry) => entry.update(source.map(From::from)),
                Entry::Vacant(entry) => {
                    entry.insert_entry(source.map(From::from));
                }
            }
        }

        Ok(())
    }
}

fn serialize(output: &str, source: Option<&str>) -> Vec<u8> {
    output
        .bytes()
        .chain(
            source
                .into_iter()
                .flat_map(|source| [COLUMN_SEPARATOR].into_iter().chain(source.bytes())),
        )
        .chain([LINE_TERMINATOR])
        .collect()
}

fn deserialize(line: &[u8]) -> Result<(&str, Option<&str>), DatabaseError> {
    let line = str::from_utf8(line)?;

    Ok(line
        .split_once(char::from(COLUMN_SEPARATOR))
        .map_or((line, None), |(output, source)| (output, Some(source))))
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

        log.set("foo", None).await.unwrap();

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

        log.set("foo", None).await.unwrap();
        log.set("bar", None).await.unwrap();

        assert_eq!(get_outputs(&log), ["bar", "foo"]);
    }

    #[tokio::test]
    async fn set_output_twice() {
        let (log, _directory) = open().await;

        log.set("foo", None).await.unwrap();
        log.set("foo", None).await.unwrap();

        assert_eq!(log.get(), ["foo"]);
    }

    #[tokio::test]
    async fn set_output_in_directory() {
        let (log, _directory) = open().await;

        log.set("foo/bar baz.o", None).await.unwrap();

        assert_eq!(log.get(), ["foo/bar baz.o"]);
    }

    #[tokio::test]
    async fn set_output_with_source() {
        let (log, _directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();

        assert_eq!(log.get(), ["foo"]);
    }

    #[tokio::test]
    async fn get_source() {
        let (log, _directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();

        assert_eq!(log.get_source("foo"), Some("bar".into()));
    }

    #[tokio::test]
    async fn get_no_source() {
        let (log, _directory) = open().await;

        log.set("foo", None).await.unwrap();

        assert_eq!(log.get_source("foo"), None);
    }

    #[tokio::test]
    async fn get_no_source_of_missing_output() {
        let (log, _directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();

        assert_eq!(log.get_source("baz"), None);
    }

    #[tokio::test]
    async fn update_source() {
        let (log, _directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();
        log.set("foo", Some("baz")).await.unwrap();

        assert_eq!(log.get_source("foo"), Some("baz".into()));
    }

    #[tokio::test]
    async fn remove_source() {
        let (log, _directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();
        log.set("foo", None).await.unwrap();

        assert_eq!(log.get(), ["foo"]);
        assert_eq!(log.get_source("foo"), None);
    }

    #[tokio::test]
    async fn set_sources() {
        let (log, _directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();
        log.set("baz", Some("qux")).await.unwrap();

        assert_eq!(log.get_source("foo"), Some("bar".into()));
        assert_eq!(log.get_source("baz"), Some("qux".into()));
    }

    #[tokio::test]
    async fn set_same_source_of_outputs() {
        let (log, _directory) = open().await;

        log.set("foo", Some("baz")).await.unwrap();
        log.set("bar", Some("baz")).await.unwrap();

        assert_eq!(log.get_source("foo"), Some("baz".into()));
        assert_eq!(log.get_source("bar"), Some("baz".into()));
    }

    #[tokio::test]
    async fn set_source_in_directory() {
        let (log, _directory) = open().await;

        log.set("foo/bar baz.o", Some("foo/bar baz.c"))
            .await
            .unwrap();

        assert_eq!(
            log.get_source("foo/bar baz.o"),
            Some("foo/bar baz.c".into())
        );
    }

    #[tokio::test]
    async fn set_empty_source() {
        let (log, _directory) = open().await;

        log.set("foo", Some("")).await.unwrap();

        assert_eq!(log.get_source("foo"), Some("".into()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_outputs_concurrently() {
        const OUTPUT_COUNT: usize = 1024;

        let (log, directory) = open().await;
        let log = Arc::new(log);
        let outputs = (0..OUTPUT_COUNT)
            .map(|index| format!("{index:04}"))
            .collect::<Vec<_>>();

        for result in try_join_all(outputs.iter().map(|output| {
            let log = log.clone();
            let output = output.clone();

            spawn(async move { log.set(&output, Some(&format!("{output}.c"))).await })
        }))
        .await
        .unwrap()
        {
            result.unwrap();
        }

        assert_eq!(get_outputs(&log), outputs);

        drop(log);

        let log = reopen(&directory).await;

        assert_eq!(get_outputs(&log), outputs);

        for output in outputs {
            assert_eq!(log.get_source(&output), Some(format!("{output}.c")));
        }
    }

    #[tokio::test]
    async fn write_line() {
        let (log, directory) = open().await;

        log.set("foo", None).await.unwrap();

        assert_eq!(read_log(&directory), b"foo\n");
    }

    #[tokio::test]
    async fn write_line_with_source() {
        let (log, directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\n");
    }

    #[tokio::test]
    async fn write_line_in_utf8() {
        let (log, directory) = open().await;

        log.set("😄", Some("🚀")).await.unwrap();

        assert_eq!(
            read_log(&directory),
            [0xf0, 0x9f, 0x98, 0x84, 0, 0xf0, 0x9f, 0x9a, 0x80, b'\n']
        );
    }

    #[tokio::test]
    async fn append_lines() {
        let (log, directory) = open().await;

        log.set("foo", None).await.unwrap();
        log.set("bar", None).await.unwrap();

        assert_eq!(read_log(&directory), b"foo\nbar\n");
    }

    #[tokio::test]
    async fn append_lines_with_sources() {
        let (log, directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();
        log.set("baz", Some("qux")).await.unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\nbaz\0qux\n");
    }

    #[tokio::test]
    async fn append_line_of_updated_source() {
        let (log, directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();
        log.set("foo", Some("baz")).await.unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\nfoo\0baz\n");
    }

    #[tokio::test]
    async fn append_line_of_removed_source() {
        let (log, directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();
        log.set("foo", None).await.unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\nfoo\n");
    }

    #[tokio::test]
    async fn append_no_line_of_same_output() {
        let (log, directory) = open().await;

        log.set("foo", None).await.unwrap();
        log.set("bar", None).await.unwrap();
        log.set("foo", None).await.unwrap();

        assert_eq!(read_log(&directory), b"foo\nbar\n");
    }

    #[tokio::test]
    async fn append_no_line_of_same_source() {
        let (log, directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();
        log.set("baz", Some("qux")).await.unwrap();
        log.set("foo", Some("bar")).await.unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\nbaz\0qux\n");
    }

    #[tokio::test]
    async fn reopen_log() {
        let (log, directory) = open().await;

        log.set("foo", None).await.unwrap();
        log.set("bar", None).await.unwrap();

        drop(log);

        assert_eq!(get_outputs(&reopen(&directory).await), ["bar", "foo"]);
    }

    #[tokio::test]
    async fn reopen_log_with_sources() {
        let (log, directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();
        log.set("baz", Some("qux")).await.unwrap();

        drop(log);

        let log = reopen(&directory).await;

        assert_eq!(get_outputs(&log), ["baz", "foo"]);
        assert_eq!(log.get_source("foo"), Some("bar".into()));
        assert_eq!(log.get_source("baz"), Some("qux".into()));
    }

    #[tokio::test]
    async fn reopen_log_with_updated_source() {
        let (log, directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();
        log.set("foo", Some("baz")).await.unwrap();

        drop(log);

        assert_eq!(
            reopen(&directory).await.get_source("foo"),
            Some("baz".into())
        );
    }

    #[tokio::test]
    async fn reopen_log_with_removed_source() {
        let (log, directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();
        log.set("foo", None).await.unwrap();

        drop(log);

        let log = reopen(&directory).await;

        assert_eq!(log.get(), ["foo"]);
        assert_eq!(log.get_source("foo"), None);
    }

    #[tokio::test]
    async fn reopen_log_in_utf8() {
        let (log, directory) = open().await;

        log.set("😄", Some("🚀")).await.unwrap();

        drop(log);

        let log = reopen(&directory).await;

        assert_eq!(log.get(), ["😄"]);
        assert_eq!(log.get_source("😄"), Some("🚀".into()));
    }

    #[tokio::test]
    async fn reopen_log_with_empty_line() {
        let directory = tempdir().unwrap();

        write_log(&directory, "foo\n\nbar\n");

        assert_eq!(get_outputs(&reopen(&directory).await), ["", "bar", "foo"]);
    }

    #[tokio::test]
    async fn reopen_log_with_empty_source() {
        let (log, directory) = open().await;

        log.set("foo", Some("")).await.unwrap();

        drop(log);

        assert_eq!(reopen(&directory).await.get_source("foo"), Some("".into()));
    }

    #[tokio::test]
    async fn reopen_log_with_column_separator_in_source() {
        let directory = tempdir().unwrap();

        write_log(&directory, "foo\0bar\0baz\n");

        assert_eq!(
            reopen(&directory).await.get_source("foo"),
            Some("bar\0baz".into())
        );
    }

    #[tokio::test]
    async fn append_lines_after_reopen() {
        let (log, directory) = open().await;

        log.set("foo", None).await.unwrap();

        drop(log);

        reopen(&directory)
            .await
            .set("bar", Some("baz"))
            .await
            .unwrap();

        assert_eq!(read_log(&directory), b"foo\nbar\0baz\n");
    }

    #[tokio::test]
    async fn append_line_of_updated_source_after_reopen() {
        let (log, directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();

        drop(log);

        reopen(&directory)
            .await
            .set("foo", Some("baz"))
            .await
            .unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\nfoo\0baz\n");
    }

    #[tokio::test]
    async fn append_no_line_of_same_output_after_reopen() {
        let (log, directory) = open().await;

        log.set("foo", None).await.unwrap();

        drop(log);

        reopen(&directory).await.set("foo", None).await.unwrap();

        assert_eq!(read_log(&directory), b"foo\n");
    }

    #[tokio::test]
    async fn append_no_line_of_same_source_after_reopen() {
        let (log, directory) = open().await;

        log.set("foo", Some("bar")).await.unwrap();

        drop(log);

        reopen(&directory)
            .await
            .set("foo", Some("bar"))
            .await
            .unwrap();

        assert_eq!(read_log(&directory), b"foo\0bar\n");
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
    async fn fail_to_open_log_with_source_in_invalid_utf8() {
        let directory = tempdir().unwrap();

        write_log(&directory, [b'f', 0, 0xff, b'\n']);

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
        async fn compact_sources() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\0bar\nfoo\0bar\nfoo\0bar\nfoo\0baz\n");

            let log = reopen(&directory).await;

            assert_eq!(log.get_source("foo"), Some("baz".into()));
            assert_eq!(read_log(&directory), b"foo\0baz\n");
        }

        #[tokio::test]
        async fn compact_output_with_source() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\nfoo\nfoo\nfoo\nfoo\nfoo\0bar\n");

            let log = reopen(&directory).await;

            assert_eq!(log.get_source("foo"), Some("bar".into()));
            assert_eq!(read_log(&directory), b"foo\0bar\n");
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
        async fn compact_after_appending_lines() {
            let (log, directory) = open().await;

            for source in ["bar", "baz", "bar", "baz"] {
                log.set("foo", Some(source)).await.unwrap();
            }

            drop(log);

            assert_eq!(
                read_log(&directory),
                b"foo\0bar\nfoo\0baz\nfoo\0bar\nfoo\0baz\n"
            );

            let log = reopen(&directory).await;

            assert_eq!(log.get_source("foo"), Some("baz".into()));
            assert_eq!(read_log(&directory), b"foo\0baz\n");
        }

        #[tokio::test]
        async fn append_line_after_compaction() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\nfoo\nfoo\nfoo\n");

            reopen(&directory).await.set("bar", None).await.unwrap();

            assert_eq!(read_log(&directory), b"foo\nbar\n");
        }

        #[tokio::test]
        async fn keep_log_of_optimal_size() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\nbar\0baz\n");

            reopen(&directory).await;

            assert_eq!(read_log(&directory), b"foo\nbar\0baz\n");
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
        async fn keep_log_of_compaction_ratio_with_sources() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\0bar\nfoo\0bar\nfoo\0baz\n");

            let log = reopen(&directory).await;

            assert_eq!(log.get_source("foo"), Some("baz".into()));
            assert_eq!(read_log(&directory), b"foo\0bar\nfoo\0bar\nfoo\0baz\n");
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
        async fn remove_incomplete_line_with_source() {
            let directory = tempdir().unwrap();

            write_log(&directory, "foo\0bar\nbaz\0qu");

            let log = reopen(&directory).await;

            assert_eq!(log.get(), ["foo"]);
            assert_eq!(log.get_source("foo"), Some("bar".into()));
            assert_eq!(read_log(&directory), b"foo\0bar\n");
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

            reopen(&directory).await.set("bar", None).await.unwrap();

            assert_eq!(read_log(&directory), b"foo\nbar\n");
            assert_eq!(get_outputs(&reopen(&directory).await), ["bar", "foo"]);
        }
    }
}
