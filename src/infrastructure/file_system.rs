mod error;
#[cfg(test)]
mod fake;
mod metadata;
mod os;

#[cfg(test)]
pub use self::fake::FakeFileSystem;
pub use self::{error::FileError, metadata::Metadata, os::OsFileSystem};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

/// A file system.
#[async_trait]
pub trait FileSystem {
    /// Reads a file.
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, FileError>;
    /// Reads a file into a string.
    async fn read_file_to_string(&self, path: &Path) -> Result<String, FileError>;
    /// Checks if a path exists.
    async fn exists(&self, path: &Path) -> Result<bool, FileError>;
    /// Returns metadata of a file if it exists.
    async fn metadata(&self, path: &Path) -> Result<Option<Metadata>, FileError>;
    /// Creates a directory and its ancestors.
    async fn create_directory(&self, path: &Path) -> Result<(), FileError>;
    /// Canonicalizes a path.
    async fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, FileError>;
    /// Removes a file.
    async fn remove_file(&self, path: &Path) -> Result<(), FileError>;
}
