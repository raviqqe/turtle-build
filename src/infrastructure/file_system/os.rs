use super::Metadata;
use crate::infrastructure::FileSystem;
use async_trait::async_trait;
use core::error::Error;
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::{
    fs::{self, File},
    io::AsyncReadExt,
    sync::Semaphore,
};

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

    async fn read_file(&self, path: &Path, buffer: &mut Vec<u8>) -> Result<(), Box<dyn Error>> {
        File::open(path)
            .await
            .map_err(|error| Self::error(error, path))?
            .read_to_end(buffer)
            .await
            .map_err(|error| Self::error(error, path))?;

        Ok(())
    }

    async fn read_file_to_string(
        &self,
        path: &Path,
        buffer: &mut String,
    ) -> Result<(), Box<dyn Error>> {
        File::open(path)
            .await
            .map_err(|error| Self::error(error, path))?
            .read_to_string(buffer)
            .await
            .map_err(|error| Self::error(error, path))?;

        Ok(())
    }

    fn error(error: io::Error, path: &Path) -> String {
        format!("{}: {}", error, path.display())
    }
}

#[async_trait]
impl FileSystem for OsFileSystem {
    async fn read_file(&self, path: &Path, buffer: &mut Vec<u8>) -> Result<(), Box<dyn Error>> {
        let _permit = self.semaphore.acquire().await?;

        self.read_file(path, buffer).await
    }

    async fn read_file_to_string(
        &self,
        path: &Path,
        buffer: &mut String,
    ) -> Result<(), Box<dyn Error>> {
        let _permit = self.semaphore.acquire().await?;

        self.read_file_to_string(path, buffer).await
    }

    async fn exists(&self, path: &Path) -> Result<bool, Box<dyn Error>> {
        Ok(fs::try_exists(path)
            .await
            .map_err(|error| Self::error(error, path))?)
    }

    async fn metadata(&self, path: &Path) -> Result<Metadata, Box<dyn Error>> {
        Ok(fs::metadata(path)
            .await
            .map_err(|error| Self::error(error, path))?
            .try_into()?)
    }

    async fn create_directory(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        fs::create_dir_all(path)
            .await
            .map_err(|error| Self::error(error, path))?;

        Ok(())
    }

    async fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, Box<dyn Error>> {
        Ok(fs::canonicalize(path)
            .await
            .map_err(|error| Self::error(error, path))?)
    }

    async fn remove_file(&self, path: &Path) -> Result<(), Box<dyn Error>> {
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
    async fn read_file_to_string() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("foo");
        let mut buffer = "foo".to_owned();

        fs::write(&path, "bar").unwrap();

        FileSystem::read_file_to_string(&OsFileSystem::new(1), &path, &mut buffer)
            .await
            .unwrap();

        assert_eq!(buffer, "foobar");
    }

    #[tokio::test]
    async fn fail_to_read_missing_file_to_string() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("foo");

        assert!(
            FileSystem::read_file_to_string(&OsFileSystem::new(1), &path, &mut String::new())
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
