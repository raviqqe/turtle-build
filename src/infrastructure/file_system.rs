mod error;
#[cfg(test)]
mod fake;
mod metadata;
mod os;

#[cfg(test)]
pub use self::fake::FakeFileSystem;
pub use self::{error::FileError, os::OsFileSystem};
use async_trait::async_trait;
use metadata::Metadata;
use std::path::{Path, PathBuf};

#[async_trait]
pub trait FileSystem {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, FileError>;
    async fn read_file_to_string(&self, path: &Path) -> Result<String, FileError>;
    async fn exists(&self, path: &Path) -> Result<bool, FileError>;
    async fn metadata(&self, path: &Path) -> Result<Metadata, FileError>;
    async fn create_directory(&self, path: &Path) -> Result<(), FileError>;
    async fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, FileError>;
    async fn remove_file(&self, path: &Path) -> Result<(), FileError>;
}
