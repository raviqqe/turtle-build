use std::{
    fs::File,
    io::{self, ErrorKind},
    path::Path,
};
use tokio::fs::{OpenOptions, read, rename, write};

pub const COLUMN_SEPARATOR: u8 = b'\0';
pub const COMPACTION_RATIO: usize = 3;
pub const LINE_TERMINATOR: u8 = b'\n';
const TEMPORARY_EXTENSION: &str = "tmp";

pub async fn read_file(path: &Path) -> Result<Vec<u8>, io::Error> {
    read(path).await.or_else(|error| {
        if error.kind() == ErrorKind::NotFound {
            Ok(vec![])
        } else {
            Err(error)
        }
    })
}

pub async fn compact_file(path: &Path, bytes: Vec<u8>) -> Result<(), io::Error> {
    let temporary_path = path.with_added_extension(TEMPORARY_EXTENSION);

    write(&temporary_path, bytes).await?;
    rename(temporary_path, path).await
}

pub async fn open_file(path: &Path) -> Result<File, io::Error> {
    Ok(OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .await?
        .into_std()
        .await)
}

pub async fn open_log(path: &Path, bytes: Option<Vec<u8>>) -> Result<File, io::Error> {
    if let Some(bytes) = bytes {
        compact_file(path, bytes).await?;
    }

    open_file(path).await
}

pub fn split_lines(bytes: &[u8]) -> impl DoubleEndedIterator<Item = &[u8]> {
    bytes
        .split_inclusive(|&byte| byte == LINE_TERMINATOR)
        .filter_map(|line| line.strip_suffix(&[LINE_TERMINATOR]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::{
        fs::{exists, read, write},
        io::Write,
    };
    use tempfile::tempdir;

    const FILENAME: &str = "foo";

    #[tokio::test]
    async fn read_existing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar").unwrap();

        assert_eq!(read_file(&path).await.unwrap(), b"bar");
    }

    #[tokio::test]
    async fn read_missing_file() {
        let directory = tempdir().unwrap();

        assert_eq!(
            read_file(&directory.path().join(FILENAME)).await.unwrap(),
            b""
        );
    }

    #[tokio::test]
    async fn fail_to_read_directory() {
        let directory = tempdir().unwrap();

        assert!(read_file(directory.path()).await.is_err());
    }

    #[tokio::test]
    async fn compact_existing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar").unwrap();

        compact_file(&path, b"baz".into()).await.unwrap();

        assert_eq!(read(&path).unwrap(), b"baz");
    }

    #[tokio::test]
    async fn compact_missing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        compact_file(&path, b"bar".into()).await.unwrap();

        assert_eq!(read(&path).unwrap(), b"bar");
    }

    #[tokio::test]
    async fn remove_temporary_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar").unwrap();

        compact_file(&path, b"baz".into()).await.unwrap();

        assert!(!exists(path.with_added_extension(TEMPORARY_EXTENSION)).unwrap());
    }

    #[tokio::test]
    async fn overwrite_temporary_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar").unwrap();
        write(path.with_added_extension(TEMPORARY_EXTENSION), "qux").unwrap();

        compact_file(&path, b"baz".into()).await.unwrap();

        assert_eq!(read(&path).unwrap(), b"baz");
        assert!(!exists(path.with_added_extension(TEMPORARY_EXTENSION)).unwrap());
    }

    #[tokio::test]
    async fn open_existing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar\n").unwrap();

        open_file(&path).await.unwrap().write_all(b"baz\n").unwrap();

        assert_eq!(read(&path).unwrap(), b"bar\nbaz\n");
    }

    #[tokio::test]
    async fn open_missing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        open_file(&path).await.unwrap();

        assert_eq!(read(&path).unwrap(), b"");
    }

    #[tokio::test]
    async fn fail_to_open_file_in_missing_directory() {
        let directory = tempdir().unwrap();

        assert!(
            open_file(&directory.path().join("foo").join(FILENAME))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn open_log_without_compaction() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar\n").unwrap();

        open_log(&path, None)
            .await
            .unwrap()
            .write_all(b"baz\n")
            .unwrap();

        assert_eq!(read(&path).unwrap(), b"bar\nbaz\n");
    }

    #[tokio::test]
    async fn open_log_with_compaction() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar\n").unwrap();

        open_log(&path, Some(b"foo\n".into()))
            .await
            .unwrap()
            .write_all(b"baz\n")
            .unwrap();

        assert_eq!(read(&path).unwrap(), b"foo\nbaz\n");
    }

    #[tokio::test]
    async fn open_missing_log() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        open_log(&path, None).await.unwrap();

        assert_eq!(read(&path).unwrap(), b"");
    }

    #[tokio::test]
    async fn open_missing_log_with_compaction() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        open_log(&path, Some(b"foo\n".into())).await.unwrap();

        assert_eq!(read(&path).unwrap(), b"foo\n");
    }

    #[test]
    fn split_no_line() {
        assert_eq!(split_lines(b"").collect::<Vec<_>>(), [b""; 0]);
    }

    #[test]
    fn split_line() {
        assert_eq!(split_lines(b"foo\n").collect::<Vec<_>>(), [b"foo"]);
    }

    #[test]
    fn split_many_lines() {
        assert_eq!(
            split_lines(b"foo\nbar\nbaz\n").collect::<Vec<_>>(),
            [b"foo", b"bar", b"baz"]
        );
    }

    #[test]
    fn split_empty_line() {
        assert_eq!(
            split_lines(b"foo\n\nbar\n").collect::<Vec<_>>(),
            [b"foo".as_slice(), b"", b"bar"]
        );
    }

    #[test]
    fn split_no_incomplete_line() {
        assert_eq!(split_lines(b"foo\nba").collect::<Vec<_>>(), [b"foo"]);
    }

    #[test]
    fn split_only_incomplete_line() {
        assert_eq!(split_lines(b"fo").collect::<Vec<_>>(), [b""; 0]);
    }

    #[test]
    fn split_lines_in_reverse() {
        assert_eq!(
            split_lines(b"foo\nbar\nba").rev().collect::<Vec<_>>(),
            [b"bar", b"foo"]
        );
    }
}
