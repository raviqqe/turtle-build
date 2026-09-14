#[cfg(test)]
mod fake;
mod metadata;

#[cfg(test)]
pub use self::fake::FakeFileSystem;
use async_trait::async_trait;
use core::{error::Error, fmt::Debug, iter, mem};
use futures::{TryFutureExt, future::try_join_all};
use metadata::Metadata;
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};
use tokio::{sync::Semaphore, task::spawn_blocking};

const BUFFER_CAPACITY: usize = 1 << 10;
const CHUNK_SIZE: usize = 64;
// File system operations mostly wait for I/O, so this limit is independent of the CPU count.
const BLOCKING_TASK_LIMIT: usize = 64;

/// A result of an operation on a file where `None` means that the file does not exist.
pub type FileResult<T> = Result<Option<T>, Box<dyn Error + Send + Sync>>;

/// A function to hash file contents.
pub type HashFunction = fn(&[u8]) -> u64;

#[async_trait]
pub trait FileSystem {
    async fn read_file_to_string(
        &self,
        path: &Path,
        buffer: &mut String,
    ) -> Result<(), Box<dyn Error>>;
    async fn exists(&self, path: &Path) -> Result<bool, Box<dyn Error>>;
    async fn metadata(&self, path: &Path) -> Result<Metadata, Box<dyn Error>>;
    async fn modified_times(
        &self,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<FileResult<SystemTime>>, Box<dyn Error>>;
    async fn hash_files(
        &self,
        paths: Vec<PathBuf>,
        hash: HashFunction,
    ) -> Result<Vec<FileResult<u64>>, Box<dyn Error>>;
    async fn create_directory(&self, path: &Path) -> Result<(), Box<dyn Error>>;
    async fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, Box<dyn Error>>;
    async fn remove_file(&self, path: &Path) -> Result<(), Box<dyn Error>>;
}

/// A file system backed by an operating system.
#[derive(Debug)]
pub struct OsFileSystem {
    semaphore: Semaphore,
    blocking_semaphore: Semaphore,
}

impl OsFileSystem {
    /// Creates a file system.
    pub fn new(open_file_limit: usize) -> Self {
        Self {
            semaphore: Semaphore::new(open_file_limit.min(Semaphore::MAX_PERMITS)),
            blocking_semaphore: Semaphore::new(BLOCKING_TASK_LIMIT),
        }
    }

    async fn run_blocking<T: Send + 'static>(
        &self,
        function: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, Box<dyn Error>> {
        let _permit = self.blocking_semaphore.acquire().await?;

        Ok(spawn_blocking(function).await?)
    }

    fn read_modified_time(path: &Path) -> FileResult<SystemTime> {
        match fs::metadata(path) {
            Ok(metadata) => Ok(Some(
                metadata
                    .modified()
                    .map_err(|error| Self::error(error, path))?,
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Self::error(error, path).into()),
        }
    }

    fn hash_file(path: &Path, buffer: &mut Vec<u8>, hash: HashFunction) -> FileResult<u64> {
        buffer.clear();

        match File::open(path) {
            Ok(mut file) => {
                file.read_to_end(buffer)
                    .map_err(|error| Self::error(error, path))?;

                Ok(Some(hash(buffer)))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Self::error(error, path).into()),
        }
    }

    fn error(error: io::Error, path: &Path) -> String {
        format!("{}: {}", error, path.display())
    }
}

#[async_trait]
impl FileSystem for OsFileSystem {
    async fn read_file_to_string(
        &self,
        path: &Path,
        buffer: &mut String,
    ) -> Result<(), Box<dyn Error>> {
        let _permit = self.semaphore.acquire().await?;
        let path = path.to_owned();
        let mut content = mem::take(buffer);

        let (result, content) = self
            .run_blocking(move || {
                let result = File::open(&path)
                    .and_then(|mut file| file.read_to_string(&mut content))
                    .map_err(|error| Self::error(error, &path));

                (result, content)
            })
            .await?;

        *buffer = content;
        result?;

        Ok(())
    }

    async fn exists(&self, path: &Path) -> Result<bool, Box<dyn Error>> {
        let path = path.to_owned();

        Ok(self
            .run_blocking(move || path.try_exists().map_err(|error| Self::error(error, &path)))
            .await??)
    }

    async fn metadata(&self, path: &Path) -> Result<Metadata, Box<dyn Error>> {
        let path = path.to_owned();

        Ok(self
            .run_blocking(move || {
                fs::metadata(&path)
                    .map(Metadata::from)
                    .map_err(|error| Self::error(error, &path))
            })
            .await??)
    }

    async fn modified_times(
        &self,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<FileResult<SystemTime>>, Box<dyn Error>> {
        run_in_chunks(paths, |paths| {
            self.run_blocking(move || {
                paths
                    .iter()
                    .map(|path| Self::read_modified_time(path))
                    .collect()
            })
        })
        .await
    }

    async fn hash_files(
        &self,
        paths: Vec<PathBuf>,
        hash: HashFunction,
    ) -> Result<Vec<FileResult<u64>>, Box<dyn Error>> {
        run_in_chunks(paths, |paths| async move {
            let _permit = self.semaphore.acquire().await?;

            self.run_blocking(move || {
                let mut buffer = Vec::with_capacity(BUFFER_CAPACITY);

                paths
                    .iter()
                    .map(|path| Self::hash_file(path, &mut buffer, hash))
                    .collect()
            })
            .await
        })
        .await
    }

    async fn create_directory(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        let path = path.to_owned();

        Ok(self
            .run_blocking(move || {
                fs::create_dir_all(&path).map_err(|error| Self::error(error, &path))
            })
            .await??)
    }

    async fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, Box<dyn Error>> {
        let path = path.to_owned();

        Ok(self
            .run_blocking(move || {
                fs::canonicalize(&path).map_err(|error| Self::error(error, &path))
            })
            .await??)
    }

    async fn remove_file(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        let path = path.to_owned();

        Ok(self
            .run_blocking(move || fs::remove_file(&path).map_err(|error| Self::error(error, &path)))
            .await??)
    }
}

// Splits paths into chunks so that a blocking task does not block others for long.
async fn run_in_chunks<T, F: Future<Output = Result<Vec<T>, Box<dyn Error>>>>(
    paths: Vec<PathBuf>,
    run: impl Fn(Vec<PathBuf>) -> F,
) -> Result<Vec<T>, Box<dyn Error>> {
    let mut paths = paths.into_iter();

    Ok(try_join_all(
        iter::from_fn(|| {
            Some(paths.by_ref().take(CHUNK_SIZE).collect::<Vec<_>>())
                .filter(|chunk| !chunk.is_empty())
        })
        // Stringify errors to keep this future sendable.
        .map(|chunk| run(chunk).map_err(stringify_error)),
    )
    .await?
    .into_iter()
    .flatten()
    .collect())
}

fn stringify_error(error: Box<dyn Error + '_>) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;
    use tempfile::tempdir;

    fn sum_bytes(content: &[u8]) -> u64 {
        content.iter().map(|&byte| u64::from(byte)).sum()
    }

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

    #[test]
    fn limit_blocking_tasks() {
        assert_eq!(
            OsFileSystem::new(1).blocking_semaphore.available_permits(),
            BLOCKING_TASK_LIMIT
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
    async fn get_modified_times() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("foo");

        fs::write(&path, "").unwrap();

        assert_eq!(
            OsFileSystem::new(1)
                .modified_times(vec![
                    directory.path().join("bar"),
                    path.clone(),
                    directory.path().join("baz"),
                ])
                .await
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .collect::<Vec<_>>(),
            [
                None,
                Some(fs::metadata(&path).unwrap().modified().unwrap()),
                None
            ]
        );
    }

    #[tokio::test]
    async fn get_no_modified_times() {
        assert!(
            OsFileSystem::new(1)
                .modified_times(vec![])
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fail_to_get_modified_time_under_file() {
        let directory = tempdir().unwrap();
        let file = directory.path().join("foo");
        let path = file.join("bar");

        fs::write(&file, "").unwrap();

        assert!(
            OsFileSystem::new(1)
                .modified_times(vec![path.clone()])
                .await
                .unwrap()
                .remove(0)
                .unwrap_err()
                .to_string()
                .contains(&path.display().to_string())
        );
    }

    #[tokio::test]
    async fn hash_files() {
        let directory = tempdir().unwrap();
        let foo = directory.path().join("foo");
        let bar = directory.path().join("bar");

        fs::write(&foo, "foo").unwrap();
        fs::write(&bar, "bar").unwrap();

        assert_eq!(
            OsFileSystem::new(1)
                .hash_files(vec![foo, directory.path().join("baz"), bar], sum_bytes)
                .await
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .collect::<Vec<_>>(),
            [Some(sum_bytes(b"foo")), None, Some(sum_bytes(b"bar"))]
        );
    }

    #[tokio::test]
    async fn get_modified_times_in_chunks() {
        let directory = tempdir().unwrap();
        let paths = (0..2 * CHUNK_SIZE + 1)
            .map(|index| directory.path().join(index.to_string()))
            .collect::<Vec<_>>();

        for path in paths.iter().step_by(3) {
            fs::write(path, "").unwrap();
        }

        assert_eq!(
            OsFileSystem::new(1)
                .modified_times(paths.clone())
                .await
                .unwrap()
                .into_iter()
                .map(|result| result.unwrap().is_some())
                .collect::<Vec<_>>(),
            (0..paths.len())
                .map(|index| index % 3 == 0)
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn hash_files_in_chunks() {
        let directory = tempdir().unwrap();
        let paths = (0..2 * CHUNK_SIZE + 1)
            .map(|index| directory.path().join(index.to_string()))
            .collect::<Vec<_>>();

        for (index, path) in paths.iter().enumerate() {
            fs::write(path, "a".repeat(index)).unwrap();
        }

        assert_eq!(
            OsFileSystem::new(1)
                .hash_files(paths.clone(), sum_bytes)
                .await
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .collect::<Vec<_>>(),
            (0..paths.len())
                .map(|index| Some(sum_bytes("a".repeat(index).as_bytes())))
                .collect::<Vec<_>>()
        );
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
