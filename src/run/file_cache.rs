use crate::infrastructure::{FileError, FileSystem, Metadata};
use alloc::sync::Arc;
use core::hash::{BuildHasher, BuildHasherDefault};
use scc::HashMap;
use std::{
    collections::hash_map::DefaultHasher,
    path::{Path, PathBuf},
};
use tokio::sync::OnceCell;

type Cache<T> = HashMap<PathBuf, Arc<OnceCell<T>>>;

pub struct FileCache {
    file_system: Arc<dyn FileSystem + Send + Sync>,
    existences: Cache<()>,
    metadata: Cache<Metadata>,
    content_hashes: Cache<u64>,
}

impl FileCache {
    pub fn new(file_system: Arc<dyn FileSystem + Send + Sync>) -> Self {
        Self {
            file_system,
            existences: HashMap::new(),
            metadata: HashMap::new(),
            content_hashes: HashMap::new(),
        }
    }

    pub async fn exists(&self, path: &Path) -> Result<bool, FileError> {
        match Self::get(&self.existences, path, || async {
            self.file_system
                .exists(path)
                .await?
                .then_some(())
                .ok_or(None)
        })
        .await
        {
            Ok(()) => Ok(true),
            Err(None) => Ok(false),
            Err(Some(error)) => Err(error),
        }
    }

    pub async fn metadata(&self, path: &Path) -> Result<Metadata, FileError> {
        Self::get(&self.metadata, path, || self.file_system.metadata(path)).await
    }

    pub async fn content_hash(&self, path: &Path) -> Result<u64, FileError> {
        Self::get(&self.content_hashes, path, || async {
            Ok(BuildHasherDefault::<DefaultHasher>::default()
                .hash_one(self.file_system.read_file(path).await?))
        })
        .await
    }

    async fn get<T: Copy, E, F: Future<Output = Result<T, E>>>(
        cache: &Cache<T>,
        path: &Path,
        fetch: impl FnOnce() -> F,
    ) -> Result<T, E> {
        // Do not inline this to avoid holding a lock of a cache across an await point.
        let cell = cache
            .entry_async(path.into())
            .await
            .or_default()
            .get()
            .clone();

        cell.get_or_try_init(fetch).await.copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::FakeFileSystem;

    fn create_cache(file_system: &FakeFileSystem) -> FileCache {
        FileCache::new(Arc::new(file_system.clone()))
    }

    mod exists {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn check_file() {
            let file_system = FakeFileSystem::default();

            file_system.write_file("foo", "");

            assert_eq!(
                create_cache(&file_system).exists("foo".as_ref()).await,
                Ok(true)
            );
        }

        #[tokio::test]
        async fn check_missing_file() {
            assert_eq!(
                create_cache(&Default::default())
                    .exists("foo".as_ref())
                    .await,
                Ok(false)
            );
        }

        #[tokio::test]
        async fn cache_existence() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");

            assert_eq!(cache.exists("foo".as_ref()).await, Ok(true));

            file_system.remove_file("foo".as_ref()).await.unwrap();

            assert_eq!(cache.exists("foo".as_ref()).await, Ok(true));
        }

        #[tokio::test]
        async fn cache_existence_per_path() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");

            assert_eq!(cache.exists("foo".as_ref()).await, Ok(true));
            assert_eq!(cache.exists("bar".as_ref()).await, Ok(false));
        }

        #[tokio::test]
        async fn do_not_cache_missing_file() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            assert_eq!(cache.exists("foo".as_ref()).await, Ok(false));

            file_system.write_file("foo", "");

            assert_eq!(cache.exists("foo".as_ref()).await, Ok(true));
        }
    }

    mod metadata {
        use super::*;
        use core::time::Duration;
        use pretty_assertions::assert_eq;
        use std::time::SystemTime;

        #[tokio::test]
        async fn get_file() {
            let file_system = FakeFileSystem::default();

            file_system.write_file("foo", "");

            assert_eq!(
                create_cache(&file_system).metadata("foo".as_ref()).await,
                Ok(Metadata::new(SystemTime::UNIX_EPOCH, false))
            );
        }

        #[tokio::test]
        async fn cache_metadata() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");

            let metadata = cache.metadata("foo".as_ref()).await.unwrap();

            file_system.write_file("foo", "");

            assert_eq!(cache.metadata("foo".as_ref()).await, Ok(metadata));
        }

        #[tokio::test]
        async fn cache_metadata_per_path() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");
            file_system.write_file("bar", "");

            assert_eq!(
                cache.metadata("foo".as_ref()).await,
                Ok(Metadata::new(SystemTime::UNIX_EPOCH, false))
            );
            assert_eq!(
                cache.metadata("bar".as_ref()).await,
                Ok(Metadata::new(
                    SystemTime::UNIX_EPOCH + Duration::from_secs(1),
                    false
                ))
            );
        }

        #[tokio::test]
        async fn fail_to_get_missing_file() {
            assert_eq!(
                create_cache(&Default::default())
                    .metadata("foo".as_ref())
                    .await,
                Err(FileError::new("file not found"))
            );
        }

        #[tokio::test]
        async fn get_file_created_after_error() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            cache.metadata("foo".as_ref()).await.unwrap_err();

            file_system.write_file("foo", "");

            assert!(cache.metadata("foo".as_ref()).await.is_ok());
        }
    }

    mod content_hash {
        use super::*;
        use pretty_assertions::{assert_eq, assert_ne};

        #[tokio::test]
        async fn hash_same_contents() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "baz");
            file_system.write_file("bar", "baz");

            assert_eq!(
                cache.content_hash("foo".as_ref()).await.unwrap(),
                cache.content_hash("bar".as_ref()).await.unwrap()
            );
        }

        #[tokio::test]
        async fn hash_different_contents() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "1");
            file_system.write_file("bar", "2");

            assert_ne!(
                cache.content_hash("foo".as_ref()).await.unwrap(),
                cache.content_hash("bar".as_ref()).await.unwrap()
            );
        }

        #[tokio::test]
        async fn cache_hash() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "1");

            let hash = cache.content_hash("foo".as_ref()).await.unwrap();

            file_system.write_file("foo", "2");

            assert_eq!(cache.content_hash("foo".as_ref()).await, Ok(hash));
        }

        #[tokio::test]
        async fn fail_to_hash_missing_file() {
            assert_eq!(
                create_cache(&Default::default())
                    .content_hash("foo".as_ref())
                    .await,
                Err(FileError::new("file not found"))
            );
        }

        #[tokio::test]
        async fn hash_file_created_after_error() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            cache.content_hash("foo".as_ref()).await.unwrap_err();

            file_system.write_file("foo", "");

            assert!(cache.content_hash("foo".as_ref()).await.is_ok());
        }
    }

    mod get {
        use super::*;
        use core::{future::pending, pin::pin, task::Poll};
        use futures::{future::join, poll};
        use pretty_assertions::assert_eq;
        use tokio::sync::Notify;

        const PATH_COUNT: usize = 64;

        #[tokio::test]
        async fn share_in_flight_fetch() {
            let cache = Cache::default();
            let notify = Notify::new();
            let mut first = pin!(FileCache::get(&cache, "foo".as_ref(), || async {
                notify.notified().await;

                Ok::<_, FileError>(1)
            }));

            assert!(poll!(&mut first).is_pending());

            let mut second = pin!(FileCache::get(&cache, "foo".as_ref(), || async {
                Ok::<_, FileError>(2)
            }));

            assert!(poll!(&mut second).is_pending());

            notify.notify_one();

            assert_eq!(join(first, second).await, (Ok(1), Ok(1)));
        }

        #[tokio::test]
        async fn fetch_again_after_cancelled_fetch() {
            let cache = Cache::default();

            assert!(
                poll!(pin!(FileCache::get(
                    &cache,
                    "foo".as_ref(),
                    pending::<Result<_, FileError>>
                )))
                .is_pending()
            );

            assert_eq!(
                FileCache::get(&cache, "foo".as_ref(), || async { Ok::<_, FileError>(1) }).await,
                Ok(1)
            );
        }

        #[tokio::test]
        async fn fetch_other_paths_during_in_flight_fetch() {
            let cache = Cache::default();
            let mut future = pin!(FileCache::get(
                &cache,
                "foo".as_ref(),
                pending::<Result<(), FileError>>
            ));

            assert!(poll!(&mut future).is_pending());

            for index in 0..PATH_COUNT {
                assert_eq!(
                    poll!(pin!(FileCache::get(
                        &cache,
                        index.to_string().as_ref(),
                        || async { Ok::<_, FileError>(()) }
                    ))),
                    Poll::Ready(Ok(()))
                );
            }
        }
    }
}
