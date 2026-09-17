use crate::infrastructure::{FileError, FileSystem};
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
    existences: Cache<bool>,
    content_hashes: Cache<u64>,
}

impl FileCache {
    pub fn new(file_system: Arc<dyn FileSystem + Send + Sync>) -> Self {
        Self {
            file_system,
            existences: HashMap::new(),
            content_hashes: HashMap::new(),
        }
    }

    pub async fn exists(&self, path: &Path) -> Result<bool, FileError> {
        Self::get(&self.existences, path, || self.file_system.exists(path)).await
    }

    pub async fn content_hash(&self, path: &Path) -> Result<u64, FileError> {
        Self::get(&self.content_hashes, path, || async {
            Ok(BuildHasherDefault::<DefaultHasher>::default()
                .hash_one(self.file_system.read_file(path).await?))
        })
        .await
    }

    async fn get<T: Copy, F: Future<Output = Result<T, FileError>>>(
        cache: &Cache<T>,
        path: &Path,
        fetch: impl FnOnce() -> F,
    ) -> Result<T, FileError> {
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

            assert_eq!(cache.exists("foo".as_ref()).await, Ok(false));

            file_system.write_file("foo", "");

            assert_eq!(cache.exists("foo".as_ref()).await, Ok(false));
        }

        #[tokio::test]
        async fn cache_existence_per_path() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            assert_eq!(cache.exists("foo".as_ref()).await, Ok(false));

            file_system.write_file("bar", "");

            assert_eq!(cache.exists("bar".as_ref()).await, Ok(true));
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

                Ok(1)
            }));

            assert!(poll!(&mut first).is_pending());

            let mut second = pin!(FileCache::get(&cache, "foo".as_ref(), || async { Ok(2) }));

            assert!(poll!(&mut second).is_pending());

            notify.notify_one();

            assert_eq!(join(first, second).await, (Ok(1), Ok(1)));
        }

        #[tokio::test]
        async fn fetch_again_after_cancelled_fetch() {
            let cache = Cache::default();

            assert!(poll!(pin!(FileCache::get(&cache, "foo".as_ref(), pending))).is_pending());

            assert_eq!(
                FileCache::get(&cache, "foo".as_ref(), || async { Ok(1) }).await,
                Ok(1)
            );
        }

        #[tokio::test]
        async fn fetch_other_paths_during_in_flight_fetch() {
            let cache = Cache::default();
            let mut future = pin!(FileCache::get(
                &cache,
                "foo".as_ref(),
                pending::<Result<(), _>>
            ));

            assert!(poll!(&mut future).is_pending());

            for index in 0..PATH_COUNT {
                assert_eq!(
                    poll!(pin!(FileCache::get(
                        &cache,
                        index.to_string().as_ref(),
                        || async { Ok(()) }
                    ))),
                    Poll::Ready(Ok(()))
                );
            }
        }
    }
}
