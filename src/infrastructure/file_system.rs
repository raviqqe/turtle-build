mod metadata;

use async_trait::async_trait;
use dashmap::DashSet;
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
    task::yield_now,
};

#[async_trait]
pub trait FileSystem {
    async fn read_file(&self, path: &Path, buffer: &mut Vec<u8>) -> Result<(), Box<dyn Error>>;
    async fn read_file_to_string(
        &self,
        path: &Path,
        buffer: &mut String,
    ) -> Result<(), Box<dyn Error>>;
    async fn metadata(&self, path: &Path) -> Result<Metadata, Box<dyn Error>>;
    async fn create_directory(&self, path: &Path) -> Result<(), Box<dyn Error>>;
    async fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, Box<dyn Error>>;
    async fn remove_file(&self, path: &Path) -> Result<(), Box<dyn Error>>;
}

// Keeps `io::Error`'s `kind()` intact (e.g. `NotFound`) while still attaching
// the path to the message, so callers can distinguish error causes instead of
// pattern-matching on `Display` output.
pub fn is_not_found(error: &(dyn Error + 'static)) -> bool {
    error
        .downcast_ref::<io::Error>()
        .is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
}

#[derive(Debug)]
pub struct OsFileSystem {
    path_lock: DashSet<PathBuf>,
    semaphore: Semaphore,
}

impl OsFileSystem {
    pub fn new(open_file_limit: usize) -> Self {
        Self {
            path_lock: DashSet::default(),
            semaphore: Semaphore::new(open_file_limit),
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

    fn error(error: io::Error, path: &Path) -> io::Error {
        io::Error::new(error.kind(), format!("{error}: {}", path.display()))
    }
}

#[async_trait]
impl FileSystem for OsFileSystem {
    async fn read_file(&self, path: &Path, buffer: &mut Vec<u8>) -> Result<(), Box<dyn Error>> {
        while !self.path_lock.insert(path.into()) {
            yield_now().await;
        }

        let permit = self.semaphore.acquire().await?;
        let result = self.read_file(path, buffer).await;
        drop(permit);

        self.path_lock.remove(path);

        result
    }

    async fn read_file_to_string(
        &self,
        path: &Path,
        buffer: &mut String,
    ) -> Result<(), Box<dyn Error>> {
        while !self.path_lock.insert(path.into()) {
            yield_now().await;
        }

        let permit = self.semaphore.acquire().await?;
        let result = self.read_file_to_string(path, buffer).await;
        drop(permit);

        self.path_lock.remove(path);

        result
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
