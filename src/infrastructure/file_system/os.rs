use super::Metadata;
use crate::infrastructure::{FileError, FileSystem};
use async_trait::async_trait;
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::{fs, sync::Semaphore};

/// A file system backed by an operating system.
#[derive(Debug)]
pub struct OsFileSystem {
    semaphore: Semaphore,
}

impl OsFileSystem {
    /// Creates a file system.
    pub fn new(open_file_limit: usize) -> Self {
        Self {
            semaphore: Semaphore::new(open_file_limit.min(Semaphore::MAX_PERMITS)),
        }
    }

    fn error(error: io::Error, path: &Path) -> FileError {
        FileError::new(format!("{}: {}", error, path.display()))
    }
}

#[async_trait]
impl FileSystem for OsFileSystem {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, FileError> {
        let _permit = self.semaphore.acquire().await?;

        fs::read(path)
            .await
            .map_err(|error| Self::error(error, path))
    }

    async fn read_file_to_string(&self, path: &Path) -> Result<String, FileError> {
        let _permit = self.semaphore.acquire().await?;

        fs::read_to_string(path)
            .await
            .map_err(|error| Self::error(error, path))
    }

    async fn exists(&self, path: &Path) -> Result<bool, FileError> {
        fs::try_exists(path)
            .await
            .map_err(|error| Self::error(error, path))
    }

    async fn metadata(&self, path: &Path) -> Result<Metadata, FileError> {
        Ok(fs::metadata(path)
            .await
            .map_err(|error| Self::error(error, path))?
            .try_into()?)
    }

    async fn create_directory(&self, path: &Path) -> Result<(), FileError> {
        fs::create_dir_all(path)
            .await
            .map_err(|error| Self::error(error, path))?;

        Ok(())
    }

    async fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, FileError> {
        fs::canonicalize(path)
            .await
            .map_err(|error| Self::error(error, path))
    }

    async fn remove_file(&self, path: &Path) -> Result<(), FileError> {
        fs::remove_file(path)
            .await
            .map_err(|error| Self::error(error, path))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn limit_open_files() {
        assert_eq!(OsFileSystem::new(42).semaphore.available_permits(), 42);
    }

    #[test]
    fn limit_open_files_to_maximum_permits() {
        assert_eq!(
            OsFileSystem::new(usize::MAX).semaphore.available_permits(),
            Semaphore::MAX_PERMITS
        );
    }

    #[tokio::test]
    async fn read_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("foo");

        fs::write(&path, "bar").unwrap();

        assert_eq!(OsFileSystem::new(1).read_file(&path).await.unwrap(), b"bar");
    }

    #[tokio::test]
    async fn fail_to_read_missing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("foo");

        assert!(
            OsFileSystem::new(1)
                .read_file(&path)
                .await
                .unwrap_err()
                .to_string()
                .contains(&path.display().to_string())
        );
    }

    #[tokio::test]
    async fn read_file_to_string() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("foo");

        fs::write(&path, "bar").unwrap();

        assert_eq!(
            OsFileSystem::new(1)
                .read_file_to_string(&path)
                .await
                .unwrap(),
            "bar"
        );
    }

    #[tokio::test]
    async fn fail_to_read_missing_file_to_string() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("foo");

        assert!(
            OsFileSystem::new(1)
                .read_file_to_string(&path)
                .await
                .unwrap_err()
                .to_string()
                .contains(&path.display().to_string())
        );
    }

    #[tokio::test]
    async fn check_file_existence() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("foo");
        let file_system = OsFileSystem::new(1);

        assert!(!file_system.exists(&path).await.unwrap());

        fs::write(&path, "").unwrap();

        assert!(file_system.exists(&path).await.unwrap());
    }

    #[tokio::test]
    async fn create_directory() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("foo").join("bar");

        OsFileSystem::new(1).create_directory(&path).await.unwrap();

        assert!(fs::metadata(&path).unwrap().is_dir());
    }

    #[tokio::test]
    async fn remove_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("foo");

        fs::write(&path, "").unwrap();

        OsFileSystem::new(1).remove_file(&path).await.unwrap();

        assert!(!path.try_exists().unwrap());
    }
}
