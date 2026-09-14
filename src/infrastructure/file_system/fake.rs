use super::{FileResult, HashFunction, Metadata};
use crate::infrastructure::FileSystem;
use alloc::sync::Arc;
use async_trait::async_trait;
use core::{
    error::Error,
    str,
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
    clock: Arc<AtomicU64>,
    modified_time_requests: Arc<Mutex<Vec<PathBuf>>>,
    content_requests: Arc<Mutex<Vec<PathBuf>>>,
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

    pub fn modified_time_requests(&self) -> Vec<PathBuf> {
        self.modified_time_requests.lock().unwrap().clone()
    }

    pub fn content_requests(&self) -> Vec<PathBuf> {
        self.content_requests.lock().unwrap().clone()
    }

    fn file(&self, path: &Path) -> Result<FakeFile, Box<dyn Error>> {
        Ok(self
            .files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or("file not found")?)
    }
}

#[async_trait]
impl FileSystem for FakeFileSystem {
    async fn read_file_to_string(
        &self,
        path: &Path,
        buffer: &mut String,
    ) -> Result<(), Box<dyn Error>> {
        buffer.push_str(str::from_utf8(&self.file(path)?.content)?);

        Ok(())
    }

    async fn exists(&self, path: &Path) -> Result<bool, Box<dyn Error>> {
        Ok(self.files.lock().unwrap().contains_key(path)
            || self.directories.lock().unwrap().contains(path))
    }

    async fn metadata(&self, path: &Path) -> Result<Metadata, Box<dyn Error>> {
        self.file(path)?;

        Ok(Metadata::new(false))
    }

    async fn modified_times(
        &self,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<FileResult<SystemTime>>, Box<dyn Error>> {
        self.modified_time_requests
            .lock()
            .unwrap()
            .extend(paths.iter().cloned());

        let files = self.files.lock().unwrap();
        let directories = self.directories.lock().unwrap();

        Ok(paths
            .iter()
            .map(|path| {
                Ok(files
                    .get(path)
                    .map(|file| file.modified_time)
                    .or_else(|| directories.contains(path).then_some(SystemTime::UNIX_EPOCH)))
            })
            .collect())
    }

    async fn hash_files(
        &self,
        paths: Vec<PathBuf>,
        hash: HashFunction,
    ) -> Result<Vec<FileResult<u64>>, Box<dyn Error>> {
        self.content_requests
            .lock()
            .unwrap()
            .extend(paths.iter().cloned());

        let files = self.files.lock().unwrap();
        let directories = self.directories.lock().unwrap();

        Ok(paths
            .iter()
            .map(|path| {
                if directories.contains(path) {
                    Err("is a directory".into())
                } else {
                    Ok(files.get(path).map(|file| hash(&file.content)))
                }
            })
            .collect())
    }

    async fn create_directory(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        self.directories.lock().unwrap().insert(path.into());

        Ok(())
    }

    async fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, Box<dyn Error>> {
        Ok(path.into())
    }

    async fn remove_file(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        self.files
            .lock()
            .unwrap()
            .remove(path)
            .ok_or("file not found")?;

        Ok(())
    }
}
