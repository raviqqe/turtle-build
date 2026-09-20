use crate::infrastructure::{FileError, FileSystem, Metadata};
use alloc::sync::Arc;
use core::hash::{BuildHasher, BuildHasherDefault};
use moka::future::Cache;
use std::collections::hash_map::DefaultHasher;

pub struct FileCache {
    file_system: Arc<dyn FileSystem + Send + Sync>,
    metadata: Cache<Arc<str>, Metadata>,
    content_hashes: Cache<Arc<str>, u64>,
}

impl FileCache {
    pub fn new(file_system: Arc<dyn FileSystem + Send + Sync>) -> Self {
        Self {
            file_system,
            metadata: Cache::builder().build(),
            content_hashes: Cache::builder().build(),
        }
    }

    pub async fn exists(&self, path: &Arc<str>) -> Result<bool, FileError> {
        Ok(self.metadata(path).await?.is_some())
    }

    pub async fn metadata(&self, path: &Arc<str>) -> Result<Option<Metadata>, FileError> {
        match self
            .metadata
            .try_get_with_by_ref(path, async {
                self.file_system
                    .metadata(path.as_ref().as_ref())
                    .await?
                    .ok_or(None)
            })
            .await
            .map_err(Arc::unwrap_or_clone)
        {
            Ok(metadata) => Ok(Some(metadata)),
            Err(None) => Ok(None),
            Err(Some(error)) => Err(error),
        }
    }

    pub async fn set_metadata(&self, path: &Arc<str>, metadata: Metadata) {
        self.metadata.entry_by_ref(path).or_insert(metadata).await;
    }

    pub async fn content_hash(&self, path: &Arc<str>) -> Result<u64, FileError> {
        self.content_hashes
            .try_get_with_by_ref(path, async {
                Ok(BuildHasherDefault::<DefaultHasher>::default()
                    .hash_one(self.file_system.read_file(path.as_ref().as_ref()).await?))
            })
            .await
            .map_err(Arc::unwrap_or_clone)
    }

    pub async fn invalidate(&self, path: &str) {
        self.metadata.invalidate(path).await;
        self.content_hashes.invalidate(path).await;
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
                create_cache(&file_system).exists(&"foo".into()).await,
                Ok(true)
            );
        }

        #[tokio::test]
        async fn check_directory() {
            let file_system = FakeFileSystem::default();

            file_system.create_directory("foo".as_ref()).await.unwrap();

            assert_eq!(
                create_cache(&file_system).exists(&"foo".into()).await,
                Ok(true)
            );
        }

        #[tokio::test]
        async fn check_missing_file() {
            assert_eq!(
                create_cache(&Default::default())
                    .exists(&"foo".into())
                    .await,
                Ok(false)
            );
        }

        #[tokio::test]
        async fn cache_existence() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");

            assert_eq!(cache.exists(&"foo".into()).await, Ok(true));

            file_system.remove_file("foo".as_ref()).await.unwrap();

            assert_eq!(cache.exists(&"foo".into()).await, Ok(true));
        }

        #[tokio::test]
        async fn cache_existence_per_path() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");

            assert_eq!(cache.exists(&"foo".into()).await, Ok(true));
            assert_eq!(cache.exists(&"bar".into()).await, Ok(false));
        }

        #[tokio::test]
        async fn do_not_cache_missing_file() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            assert_eq!(cache.exists(&"foo".into()).await, Ok(false));

            file_system.write_file("foo", "");

            assert_eq!(cache.exists(&"foo".into()).await, Ok(true));
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
                create_cache(&file_system).metadata(&"foo".into()).await,
                Ok(Some(Metadata::new(SystemTime::UNIX_EPOCH, false)))
            );
        }

        #[tokio::test]
        async fn get_missing_file() {
            assert_eq!(
                create_cache(&Default::default())
                    .metadata(&"foo".into())
                    .await,
                Ok(None)
            );
        }

        #[tokio::test]
        async fn cache_metadata() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");

            let metadata = cache.metadata(&"foo".into()).await.unwrap();

            file_system.write_file("foo", "");

            assert_eq!(cache.metadata(&"foo".into()).await, Ok(metadata));
        }

        #[tokio::test]
        async fn cache_metadata_per_path() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");
            file_system.write_file("bar", "");

            assert_eq!(
                cache.metadata(&"foo".into()).await,
                Ok(Some(Metadata::new(SystemTime::UNIX_EPOCH, false)))
            );
            assert_eq!(
                cache.metadata(&"bar".into()).await,
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

            assert_eq!(cache.exists(&"foo".into()).await, Ok(true));

            file_system.write_file("foo", "");

            assert_eq!(
                cache.metadata(&"foo".into()).await,
                Ok(Some(Metadata::new(SystemTime::UNIX_EPOCH, false)))
            );
        }

        #[tokio::test]
        async fn do_not_cache_missing_file() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            assert_eq!(cache.metadata(&"foo".into()).await, Ok(None));

            file_system.write_file("foo", "");

            assert_eq!(
                cache.metadata(&"foo".into()).await,
                Ok(Some(Metadata::new(SystemTime::UNIX_EPOCH, false)))
            );
        }
    }

    mod set_metadata {
        use super::*;
        use core::{pin::pin, task::Poll, time::Duration};
        use futures::poll;
        use pretty_assertions::assert_eq;
        use std::time::SystemTime;
        use tokio::sync::Notify;

        #[tokio::test]
        async fn set_metadata() {
            let cache = create_cache(&Default::default());
            let metadata = Metadata::new(SystemTime::UNIX_EPOCH, false);

            cache.set_metadata(&"foo".into(), metadata).await;

            assert_eq!(cache.metadata(&"foo".into()).await, Ok(Some(metadata)));
        }

        #[tokio::test]
        async fn set_metadata_per_path() {
            let cache = create_cache(&Default::default());

            cache
                .set_metadata(&"foo".into(), Metadata::new(SystemTime::UNIX_EPOCH, false))
                .await;

            assert_eq!(cache.metadata(&"bar".into()).await, Ok(None));
        }

        #[tokio::test]
        async fn keep_cached_metadata() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");

            let metadata = cache.metadata(&"foo".into()).await.unwrap();

            cache
                .set_metadata(
                    &"foo".into(),
                    Metadata::new(SystemTime::UNIX_EPOCH + Duration::from_secs(1), false),
                )
                .await;

            assert_eq!(cache.metadata(&"foo".into()).await, Ok(metadata));
        }

        #[tokio::test]
        async fn keep_metadata_of_in_flight_fetch() {
            let cache = create_cache(&Default::default());
            let path = "foo".into();
            let metadata = Metadata::new(SystemTime::UNIX_EPOCH, false);
            let notify = Notify::new();
            let mut future = pin!(cache.metadata.try_get_with_by_ref(&path, async {
                notify.notified().await;

                Ok::<_, FileError>(metadata)
            }));

            assert!(poll!(&mut future).is_pending());
            assert_eq!(
                poll!(pin!(cache.set_metadata(
                    &path,
                    Metadata::new(SystemTime::UNIX_EPOCH + Duration::from_secs(1), false),
                ))),
                Poll::Ready(())
            );

            notify.notify_one();

            assert_eq!(future.await, Ok(metadata));
            assert_eq!(cache.metadata(&path).await, Ok(Some(metadata)));
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
                cache.content_hash(&"foo".into()).await.unwrap(),
                cache.content_hash(&"bar".into()).await.unwrap()
            );
        }

        #[tokio::test]
        async fn hash_different_contents() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "1");
            file_system.write_file("bar", "2");

            assert_ne!(
                cache.content_hash(&"foo".into()).await.unwrap(),
                cache.content_hash(&"bar".into()).await.unwrap()
            );
        }

        #[tokio::test]
        async fn cache_hash() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "1");

            let hash = cache.content_hash(&"foo".into()).await.unwrap();

            file_system.write_file("foo", "2");

            assert_eq!(cache.content_hash(&"foo".into()).await, Ok(hash));
        }

        #[tokio::test]
        async fn fail_to_hash_missing_file() {
            assert_eq!(
                create_cache(&Default::default())
                    .content_hash(&"foo".into())
                    .await,
                Err(FileError::new("file not found"))
            );
        }

        #[tokio::test]
        async fn hash_file_created_after_error() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            cache.content_hash(&"foo".into()).await.unwrap_err();

            file_system.write_file("foo", "");

            assert!(cache.content_hash(&"foo".into()).await.is_ok());
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

            assert_eq!(cache.exists(&"foo".into()).await, Ok(true));

            file_system.remove_file("foo".as_ref()).await.unwrap();
            cache.invalidate("foo").await;

            assert_eq!(cache.exists(&"foo".into()).await, Ok(false));
        }

        #[tokio::test]
        async fn get_updated_metadata() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "");

            let metadata = cache.metadata(&"foo".into()).await.unwrap();

            file_system.write_file("foo", "");
            cache.invalidate("foo").await;

            assert_ne!(cache.metadata(&"foo".into()).await, Ok(metadata));
        }

        #[tokio::test]
        async fn hash_updated_content() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "1");

            let hash = cache.content_hash(&"foo".into()).await.unwrap();

            file_system.write_file("foo", "2");
            cache.invalidate("foo").await;

            assert_ne!(cache.content_hash(&"foo".into()).await, Ok(hash));
        }

        #[tokio::test]
        async fn keep_other_path() {
            let file_system = FakeFileSystem::default();
            let cache = create_cache(&file_system);

            file_system.write_file("foo", "1");
            file_system.write_file("bar", "1");

            let metadata = cache.metadata(&"bar".into()).await;
            let hash = cache.content_hash(&"bar".into()).await;

            file_system.write_file("bar", "2");
            cache.invalidate("foo").await;

            assert_eq!(cache.metadata(&"bar".into()).await, metadata);
            assert_eq!(cache.content_hash(&"bar".into()).await, hash);
        }
    }
}
