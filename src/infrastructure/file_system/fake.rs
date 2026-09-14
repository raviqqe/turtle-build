use super::Metadata;
use crate::infrastructure::FileSystem;
use async_trait::async_trait;
use alloc::sync::Arc;
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
    async fn read_file(&self, path: &Path, buffer: &mut Vec<u8>) -> Result<(), Box<dyn Error>> {
        buffer.extend(self.file(path)?.content);

        Ok(())
    }

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
        Ok(Metadata::new(self.file(path)?.modified_time, false))
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
