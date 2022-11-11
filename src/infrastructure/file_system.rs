use async_trait::async_trait;
use dashmap::DashSet;
use std::{
    error::Error,
    fmt::Debug,
    io,
    path::{Path, PathBuf},
    time::SystemTime,
};
use tokio::{
    fs::{self, File},
    io::AsyncReadExt,
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
    async fn modified_time(&self, path: &Path) -> Result<SystemTime, Box<dyn Error>>;
    async fn create_directory(&self, path: &Path) -> Result<(), Box<dyn Error>>;
    async fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, Box<dyn Error>>;
}

#[derive(Debug, Default)]
pub struct OsFileSystem {
    path_lock: DashSet<PathBuf>,
}

impl OsFileSystem {
    pub fn new() -> Self {
        Self {
            path_lock: DashSet::default(),
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
        while !self.path_lock.insert(path.into()) {
            yield_now().await;
        }

        let result = self.read_file(path, buffer).await;

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

        let result = self.read_file_to_string(path, buffer).await;

        self.path_lock.remove(path);

        result
    }

    async fn modified_time(&self, path: &Path) -> Result<SystemTime, Box<dyn Error>> {
        Ok(fs::metadata(path)
            .await
            .map_err(|error| Self::error(error, path))?
            .modified()
            .map_err(|error| Self::error(error, path))?)
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
}
