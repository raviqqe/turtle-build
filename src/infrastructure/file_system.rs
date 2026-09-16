#[cfg(test)]
mod fake;
mod metadata;
mod os;

#[cfg(test)]
pub use self::fake::FakeFileSystem;
pub use self::os::OsFileSystem;
use async_trait::async_trait;
use core::error::Error;
use metadata::Metadata;
use std::path::{Path, PathBuf};

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
