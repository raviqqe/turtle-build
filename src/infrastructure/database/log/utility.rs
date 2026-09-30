use std::{
    fs::{File, OpenOptions, read, rename, write},
    io::{self, ErrorKind},
    path::Path,
};

pub const COLUMN_SEPARATOR: u8 = b'\0';
pub const COMPACTION_RATIO: usize = 3;
pub const LINE_TERMINATOR: u8 = b'\n';
const TEMPORARY_EXTENSION: &str = "tmp";

// Log files are read and opened inline because databases open before any build
// runs, when a round trip through the blocking thread pool only adds latency.
pub fn read_file(path: &Path) -> Result<Vec<u8>, io::Error> {
    read(path).or_else(|error| {
        if error.kind() == ErrorKind::NotFound {
            Ok(vec![])
        } else {
            Err(error)
        }
    })
}

pub fn compact_file(path: &Path, bytes: Vec<u8>) -> Result<(), io::Error> {
    let temporary_path = path.with_added_extension(TEMPORARY_EXTENSION);

    write(&temporary_path, bytes)?;
    rename(temporary_path, path)
}

pub fn open_file(path: &Path) -> Result<File, io::Error> {
    OpenOptions::new().append(true).create(true).open(path)
}

pub fn open_log(path: &Path, bytes: Option<Vec<u8>>) -> Result<File, io::Error> {
    if let Some(bytes) = bytes {
        compact_file(path, bytes)?;
    }

    open_file(path)
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

    #[test]
    fn read_existing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar").unwrap();

        assert_eq!(read_file(&path).unwrap(), b"bar");
    }

    #[test]
    fn read_missing_file() {
        let directory = tempdir().unwrap();

        assert_eq!(read_file(&directory.path().join(FILENAME)).unwrap(), b"");
    }

    #[test]
    fn fail_to_read_directory() {
        let directory = tempdir().unwrap();

        assert!(read_file(directory.path()).is_err());
    }

    #[test]
    fn compact_existing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar").unwrap();

        compact_file(&path, b"baz".into()).unwrap();

        assert_eq!(read(&path).unwrap(), b"baz");
    }

    #[test]
    fn compact_missing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        compact_file(&path, b"bar".into()).unwrap();

        assert_eq!(read(&path).unwrap(), b"bar");
    }

    #[test]
    fn remove_temporary_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar").unwrap();

        compact_file(&path, b"baz".into()).unwrap();

        assert!(!exists(path.with_added_extension(TEMPORARY_EXTENSION)).unwrap());
    }

    #[test]
    fn overwrite_temporary_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar").unwrap();
        write(path.with_added_extension(TEMPORARY_EXTENSION), "qux").unwrap();

        compact_file(&path, b"baz".into()).unwrap();

        assert_eq!(read(&path).unwrap(), b"baz");
        assert!(!exists(path.with_added_extension(TEMPORARY_EXTENSION)).unwrap());
    }

    #[test]
    fn open_existing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar\n").unwrap();

        open_file(&path).unwrap().write_all(b"baz\n").unwrap();

        assert_eq!(read(&path).unwrap(), b"bar\nbaz\n");
    }

    #[test]
    fn open_missing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        open_file(&path).unwrap();

        assert_eq!(read(&path).unwrap(), b"");
    }

    #[test]
    fn fail_to_open_file_in_missing_directory() {
        let directory = tempdir().unwrap();

        assert!(open_file(&directory.path().join("foo").join(FILENAME)).is_err());
    }

    #[test]
    fn open_log_without_compaction() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar\n").unwrap();

        open_log(&path, None).unwrap().write_all(b"baz\n").unwrap();

        assert_eq!(read(&path).unwrap(), b"bar\nbaz\n");
    }

    #[test]
    fn open_log_with_compaction() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        write(&path, "bar\n").unwrap();

        open_log(&path, Some(b"foo\n".into()))
            .unwrap()
            .write_all(b"baz\n")
            .unwrap();

        assert_eq!(read(&path).unwrap(), b"foo\nbaz\n");
    }

    #[test]
    fn open_missing_log() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        open_log(&path, None).unwrap();

        assert_eq!(read(&path).unwrap(), b"");
    }

    #[test]
    fn open_missing_log_with_compaction() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FILENAME);

        open_log(&path, Some(b"foo\n".into())).unwrap();

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
