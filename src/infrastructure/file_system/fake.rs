use super::Metadata;
use crate::infrastructure::{FileError, FileSystem};
use alloc::sync::Arc;
use async_trait::async_trait;
use core::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

#[derive(Clone, Debug, Default)]
pub struct FakeFileSystem {
    files: Arc<Mutex<HashMap<PathBuf, FakeFile>>>,
    directories: Arc<Mutex<HashSet<PathBuf>>>,
    metadata_requests: Arc<Mutex<Vec<PathBuf>>>,
    metadata_failures: Arc<Mutex<HashSet<PathBuf>>>,
    clock: Arc<AtomicU64>,
}

#[derive(Clone, Debug)]
struct FakeFile {
    content: Vec<u8>,
    modified_time: SystemTime,
}

impl FakeFileSystem {
    pub fn write_file(&self, path: &str, content: &str) {
        self.files.lock().unwrap().insert(
            path.into(),
            FakeFile {
                content: content.into(),
                modified_time: SystemTime::UNIX_EPOCH
                    + Duration::from_secs(self.clock.fetch_add(1, Ordering::SeqCst)),
            },
        );
    }

    pub fn metadata_requests(&self) -> Vec<PathBuf> {
        self.metadata_requests.lock().unwrap().clone()
    }

    pub fn fail_metadata(&self, path: &str) {
        self.metadata_failures.lock().unwrap().insert(path.into());
    }

    fn file(&self, path: &Path) -> Result<FakeFile, FileError> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| FileError::new("file not found"))
    }
}

#[async_trait]
impl FileSystem for FakeFileSystem {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, FileError> {
        Ok(self.file(path)?.content)
    }

    async fn read_file_to_string(&self, path: &Path) -> Result<String, FileError> {
        String::from_utf8(self.file(path)?.content).map_err(FileError::new)
    }

    async fn exists(&self, path: &Path) -> Result<bool, FileError> {
        self.metadata_requests.lock().unwrap().push(path.into());

        Ok(self.files.lock().unwrap().contains_key(path)
            || self.directories.lock().unwrap().contains(path))
    }

    async fn metadata(&self, path: &Path) -> Result<Option<Metadata>, FileError> {
        self.metadata_requests.lock().unwrap().push(path.into());

        if self.metadata_failures.lock().unwrap().contains(path) {
            return Err(FileError::new("metadata failure"));
        }

        Ok(self
            .files
            .lock()
            .unwrap()
            .get(path)
            .map(|file| Metadata::new(file.modified_time, false))
            .or_else(|| {
                self.directories
                    .lock()
                    .unwrap()
                    .contains(path)
                    .then_some(Metadata::new(SystemTime::UNIX_EPOCH, true))
            }))
    }

    async fn create_directory(&self, path: &Path) -> Result<(), FileError> {
        self.directories.lock().unwrap().insert(path.into());

        Ok(())
    }

    async fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, FileError> {
        Ok(path.into())
    }

    async fn remove_file(&self, path: &Path) -> Result<(), FileError> {
        self.files
            .lock()
            .unwrap()
            .remove(path)
            .ok_or_else(|| FileError::new("file not found"))?;

        Ok(())
    }
}
