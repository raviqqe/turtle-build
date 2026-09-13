mod metadata;

use async_trait::async_trait;
use metadata::Metadata;
use std::{
    error::Error,
    fmt::Debug,
    io,
    path::{Path, PathBuf},
};
use tokio::{
    fs::{self, File},
    io::AsyncReadExt,
    sync::Semaphore,
};

#[async_trait]
pub trait FileSystem {
    async fn read_file(&self, path: &Path, buffer: &mut Vec<u8>) -> Result<(), Box<dyn Error>>;
    async fn read_file_to_string(
        &self,
        path: &Path,
        buffer: &mut String,
    ) -> Result<(), Box<dyn Error>>;
    async fn exists(&self, path: &Path) -> Result<bool, Box<dyn Error>>;
    async fn metadata(&self, path: &Path) -> Result<Metadata, Box<dyn Error>>;
    async fn create_directory(&self, path: &Path) -> Result<(), Box<dyn Error>>;
    async fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, Box<dyn Error>>;
    async fn remove_file(&self, path: &Path) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug)]
pub struct OsFileSystem {
    semaphore: Semaphore,
}

impl OsFileSystem {
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
}
