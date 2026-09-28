use std::{
    fs::File,
    io::{self, ErrorKind},
    path::Path,
};
use tokio::fs::{OpenOptions, read, rename, write};

pub const COMPACTION_RATIO: usize = 3;
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
}
