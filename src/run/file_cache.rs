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
    metadata: Cache<Metadata>,
    content_hashes: Cache<u64>,
}

impl FileCache {
    pub fn new(file_system: Arc<dyn FileSystem + Send + Sync>) -> Self {
        Self {
            file_system,
            metadata: HashMap::new(),
            content_hashes: HashMap::new(),
        }
    }

    pub async fn exists(&self, path: &Path) -> Result<bool, FileError> {
        Ok(self.metadata(path).await?.is_some())
    }

    pub async fn metadata(&self, path: &Path) -> Result<Option<Metadata>, FileError> {
        match Self::get(&self.metadata, path, || async {
            self.file_system.metadata(path).await?.ok_or(None)
        })
        .await
        {
            Ok(metadata) => Ok(Some(metadata)),
            Err(None) => Ok(None),
            Err(Some(error)) => Err(error),
        }
    }

    pub async fn content_hash(&self, path: &Path) -> Result<u64, FileError> {
        Self::get(&self.content_hashes, path, || async {
            Ok(BuildHasherDefault::<DefaultHasher>::default()
                .hash_one(self.file_system.read_file(path).await?))
        })
        .await
    }

    pub async fn invalidate(&self, path: &Path) {
        self.metadata.remove_async(path).await;
        self.content_hashes.remove_async(path).await;
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
        async fn check_directory() {
            let file_system = FakeFileSystem::default();

            file_system.create_directory("foo".as_ref()).await.unwrap();

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
                Ok(Some(Metadata::new(SystemTime::UNIX_EPOCH, false)))
            );
        }

        #[tokio::test]
        async fn get_missing_file() {
            assert_eq!(
                create_cache(&Default::default())
                    .metadata("foo".as_ref())
                    .await,
                Ok(None)
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
                Ok(Some(Metadata::new(SystemTime::UNIX_EPOCH, false)))
            );
            assert_eq!(
                cache.metadata("bar".as_ref()).await,
                Ok(Some(Metadata::new(
                    SystemTime::UNIX_EPOCH + Duration::from_secs(1),
                    false
                )))
            );
        }

        #[tokio::test]
        async fn cache_metadata_on_existence_check() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");

            assert_eq!(cache.exists("foo".as_ref()).await, Ok(true));

            file_system.write_file("foo", "");

            assert_eq!(
                cache.metadata("foo".as_ref()).await,
                Ok(Some(Metadata::new(SystemTime::UNIX_EPOCH, false)))
            );
        }

        #[tokio::test]
        async fn do_not_cache_missing_file() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            assert_eq!(cache.metadata("foo".as_ref()).await, Ok(None));

            file_system.write_file("foo", "");

            assert_eq!(
                cache.metadata("foo".as_ref()).await,
                Ok(Some(Metadata::new(SystemTime::UNIX_EPOCH, false)))
            );
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

    mod invalidate {
        use super::*;
        use pretty_assertions::{assert_eq, assert_ne};

        #[tokio::test]
        async fn check_removed_file() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");

            assert_eq!(cache.exists("foo".as_ref()).await, Ok(true));

            file_system.remove_file("foo".as_ref()).await.unwrap();
            cache.invalidate("foo".as_ref()).await;

            assert_eq!(cache.exists("foo".as_ref()).await, Ok(false));
        }

        #[tokio::test]
        async fn get_updated_metadata() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");

            let metadata = cache.metadata("foo".as_ref()).await.unwrap();

            file_system.write_file("foo", "");
            cache.invalidate("foo".as_ref()).await;

            assert_ne!(cache.metadata("foo".as_ref()).await, Ok(metadata));
        }

        #[tokio::test]
        async fn hash_updated_content() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "1");

            let hash = cache.content_hash("foo".as_ref()).await.unwrap();

            file_system.write_file("foo", "2");
            cache.invalidate("foo".as_ref()).await;

            assert_ne!(cache.content_hash("foo".as_ref()).await, Ok(hash));
        }

        #[tokio::test]
        async fn keep_other_path() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "1");
            file_system.write_file("bar", "1");

            let metadata = cache.metadata("bar".as_ref()).await;
            let hash = cache.content_hash("bar".as_ref()).await;

            file_system.write_file("bar", "2");
            cache.invalidate("foo".as_ref()).await;

            assert_eq!(cache.metadata("bar".as_ref()).await, metadata);
            assert_eq!(cache.content_hash("bar".as_ref()).await, hash);
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
        async fn fetch_again_after_removal_during_in_flight_fetch() {
            let cache = Cache::default();
            let notify = Notify::new();
            let mut first = pin!(FileCache::get(&cache, "foo".as_ref(), || async {
                notify.notified().await;

                Ok::<_, FileError>(1)
            }));

            assert!(poll!(&mut first).is_pending());

            cache.remove_async(Path::new("foo")).await;

            assert_eq!(
                poll!(pin!(FileCache::get(&cache, "foo".as_ref(), || async {
                    Ok::<_, FileError>(2)
                }))),
                Poll::Ready(Ok(2))
            );

            notify.notify_one();

            assert_eq!(first.await, Ok(1));
            assert_eq!(
                FileCache::get(&cache, "foo".as_ref(), || async { Ok::<_, FileError>(3) }).await,
                Ok(2)
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
