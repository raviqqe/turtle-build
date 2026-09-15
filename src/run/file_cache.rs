use crate::error::BuildError;
use alloc::sync::Arc;
use core::iter::{repeat_n, repeat_with};
use futures::future::join_all;
use itertools::Itertools;
use scc::{HashMap, hash_map::Entry};
use std::time::SystemTime;
use tokio::{spawn, sync::SetOnce};

pub type Results<T> = Vec<Result<T, BuildError>>;

// Results of one fetch in the order of its paths.
type Batch<T> = SetOnce<Results<T>>;

pub struct FileCache {
    modified_times: CacheTable<Option<SystemTime>>,
    content_hashes: CacheTable<Option<u64>>,
}

impl FileCache {
    pub fn new() -> Self {
        Self {
            modified_times: CacheTable::new(),
            content_hashes: CacheTable::new(),
        }
    }

    pub const fn modified_times(&self) -> &CacheTable<Option<SystemTime>> {
        &self.modified_times
    }

    pub const fn content_hashes(&self) -> &CacheTable<Option<u64>> {
        &self.content_hashes
    }

    pub async fn invalidate(&self, path: &str) {
        self.modified_times.invalidate(path).await;
        self.content_hashes.invalidate(path).await;
    }
}

pub struct CacheTable<T> {
    slots: HashMap<Arc<str>, Slot<T>>,
}

impl<T: Clone + Send + Sync + 'static> CacheTable<T> {
    pub fn new() -> Self {
        Self {
            slots: HashMap::new(),
        }
    }

    /// Returns one result per path in request order, fetching misses in one batch.
    pub async fn get<F>(&self, paths: &[&str], fetch: impl FnOnce(Vec<Arc<str>>) -> F) -> Results<T>
    where
        F: Future<Output = Result<Results<T>, BuildError>> + Send + 'static,
    {
        let mut batch = None;
        let mut keys = vec![];
        let mut lookups = Vec::with_capacity(paths.len());

        for path in paths {
            lookups.push(self.lookup_or_claim(path, &mut batch, &mut keys).await);
        }

        if let Some(batch) = batch {
            // Create the guard outside the detached task so that it completes the batch even
            // if the task is dropped before its first poll.
            let guard = BatchGuard {
                batch,
                length: keys.len(),
            };
            let future = fetch(keys);

            spawn(async move { guard.complete(future.await) });
        }

        join_all(pending_batches(&lookups).map(|batch| batch.wait())).await;

        lookups.into_iter().map(Lookup::into_result).collect()
    }

    pub async fn invalidate(&self, path: &str) {
        self.slots.remove_async(path).await;
    }

    async fn lookup_or_claim(
        &self,
        path: &str,
        batch: &mut Option<Arc<Batch<T>>>,
        keys: &mut Vec<Arc<str>>,
    ) -> Lookup<T> {
        if let Some(lookup) = self.slots.read_async(path, |_, slot| slot.lookup()).await {
            return lookup;
        }

        match self.slots.entry_async(path.into()).await {
            Entry::Occupied(entry) => entry.get().lookup(),
            Entry::Vacant(entry) => {
                let batch = batch.get_or_insert_with(|| Arc::new(Batch::new()));
                let index = keys.len();

                keys.push(entry.key().clone());
                entry.insert_entry(Slot {
                    batch: batch.clone(),
                    index,
                });

                Lookup::Pending(batch.clone(), index)
            }
        }
    }
}

struct Slot<T> {
    batch: Arc<Batch<T>>,
    index: usize,
}

impl<T: Clone> Slot<T> {
    fn lookup(&self) -> Lookup<T> {
        self.batch.get().map_or_else(
            || Lookup::Pending(self.batch.clone(), self.index),
            |results| Lookup::Ready(results.get(self.index).cloned().unwrap_or_else(aborted)),
        )
    }
}

enum Lookup<T> {
    Ready(Result<T, BuildError>),
    Pending(Arc<Batch<T>>, usize),
}

impl<T> Lookup<T> {
    const fn batch(&self) -> Option<&Arc<Batch<T>>> {
        match self {
            Self::Ready(_) => None,
            Self::Pending(batch, _) => Some(batch),
        }
    }
}

impl<T: Clone> Lookup<T> {
    fn into_result(self) -> Result<T, BuildError> {
        match self {
            Self::Ready(result) => result,
            Self::Pending(batch, index) => batch
                .get()
                .and_then(|results| results.get(index))
                .cloned()
                .unwrap_or_else(aborted),
        }
    }
}

// A guard that fails every path in a batch if its fetch task panics or is dropped.
struct BatchGuard<T> {
    batch: Arc<Batch<T>>,
    length: usize,
}

impl<T> BatchGuard<T> {
    fn complete(self, result: Result<Results<T>, BuildError>) {
        self.batch
            .set(match result {
                Ok(results) if results.len() == self.length => results,
                Ok(results) => self.fail(BuildError::Other(format!(
                    "file system returned {} results for {} paths",
                    results.len(),
                    self.length
                ))),
                Err(error) => self.fail(error),
            })
            .ok();
    }

    fn fail(&self, error: BuildError) -> Results<T> {
        repeat_n(error, self.length).map(Err).collect()
    }
}

impl<T> Drop for BatchGuard<T> {
    fn drop(&mut self) {
        if !self.batch.initialized() {
            self.batch
                .set(repeat_with(aborted).take(self.length).collect())
                .ok();
        }
    }
}

fn pending_batches<T>(lookups: &[Lookup<T>]) -> impl Iterator<Item = &Arc<Batch<T>>> {
    lookups
        .iter()
        .filter_map(Lookup::batch)
        .unique_by(|batch| Arc::as_ptr(batch))
}

fn aborted<T>() -> Result<T, BuildError> {
    Err(BuildError::Other("file lookup aborted".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        future::{pending, ready},
        pin::pin,
        time::Duration,
    };
    use futures::{
        FutureExt,
        future::{BoxFuture, join},
        poll,
    };
    use pretty_assertions::assert_eq;
    use std::sync::Mutex;
    use tokio::{runtime::Builder, sync::oneshot, time::timeout};

    const TIMEOUT: Duration = Duration::from_secs(10);

    type FetchResult<T> = Result<Results<T>, BuildError>;

    #[derive(Default)]
    struct Fetcher {
        requests: Arc<Mutex<Vec<Vec<String>>>>,
    }

    impl Fetcher {
        fn requests(&self) -> Vec<Vec<String>> {
            self.requests.lock().unwrap().clone()
        }

        fn fetch<T: Send + 'static>(
            &self,
            results: impl FnOnce(&[Arc<str>]) -> FetchResult<T> + Send + 'static,
        ) -> impl FnOnce(Vec<Arc<str>>) -> BoxFuture<'static, FetchResult<T>> {
            self.fetch_after(ready(()), results)
        }

        fn fetch_after<T: Send + 'static>(
            &self,
            gate: impl Future + Send + 'static,
            results: impl FnOnce(&[Arc<str>]) -> FetchResult<T> + Send + 'static,
        ) -> impl FnOnce(Vec<Arc<str>>) -> BoxFuture<'static, FetchResult<T>> {
            let requests = self.requests.clone();

            move |paths| {
                requests
                    .lock()
                    .unwrap()
                    .push(paths.iter().map(ToString::to_string).collect());

                async move {
                    gate.await;
                    results(&paths)
                }
                .boxed()
            }
        }
    }

    fn path_values(paths: &[Arc<str>]) -> FetchResult<String> {
        Ok(paths.iter().map(|path| Ok(path.to_string())).collect())
    }

    fn path_errors(paths: &[Arc<str>]) -> FetchResult<String> {
        Ok(paths
            .iter()
            .map(|path| Err(BuildError::Other(path.to_string())))
            .collect())
    }

    async fn wait<F: Future>(future: F) -> F::Output {
        timeout(TIMEOUT, future).await.unwrap()
    }

    #[tokio::test]
    async fn get_values_in_request_order() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();

        table.get(&["bar"], fetcher.fetch(path_values)).await;

        assert_eq!(
            table
                .get(&["foo", "bar", "baz", "foo"], fetcher.fetch(path_values))
                .await,
            [
                Ok("foo".into()),
                Ok("bar".into()),
                Ok("baz".into()),
                Ok("foo".into())
            ]
        );
    }

    #[tokio::test]
    async fn fetch_misses_in_one_batch() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();

        table
            .get(&["foo", "bar", "baz"], fetcher.fetch(path_values))
            .await;

        assert_eq!(fetcher.requests(), [["foo", "bar", "baz"]]);
    }

    #[tokio::test]
    async fn fetch_only_misses() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();

        table.get(&["foo"], fetcher.fetch(path_values)).await;

        assert_eq!(
            table.get(&["foo", "bar"], fetcher.fetch(path_values)).await,
            [Ok("foo".into()), Ok("bar".into())]
        );
        assert_eq!(fetcher.requests(), [["foo"], ["bar"]]);
    }

    #[tokio::test]
    async fn serve_hits_without_fetching() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();

        table.get(&["foo"], fetcher.fetch(path_values)).await;

        assert_eq!(
            table.get(&["foo"], fetcher.fetch(path_errors)).await,
            [Ok("foo".into())]
        );
        assert_eq!(fetcher.requests(), [["foo"]]);
    }

    #[tokio::test]
    async fn fetch_duplicate_path_once() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();

        assert_eq!(
            table.get(&["foo", "foo"], fetcher.fetch(path_values)).await,
            [Ok("foo".into()), Ok("foo".into())]
        );
        assert_eq!(fetcher.requests(), [["foo"]]);
    }

    #[tokio::test]
    async fn cache_missing_file() {
        let cache = FileCache::new();
        let fetcher = Fetcher::default();

        assert_eq!(
            cache
                .modified_times()
                .get(&["foo"], fetcher.fetch(|_| Ok(vec![Ok(None)])))
                .await,
            [Ok(None)]
        );
        assert_eq!(
            cache
                .modified_times()
                .get(
                    &["foo"],
                    fetcher.fetch(|_| Ok(vec![Ok(Some(SystemTime::UNIX_EPOCH))]))
                )
                .await,
            [Ok(None)]
        );
        assert_eq!(fetcher.requests(), [["foo"]]);
    }

    #[tokio::test]
    async fn cache_error_until_invalidation() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();

        table.get(&["foo"], fetcher.fetch(path_errors)).await;

        assert_eq!(
            table.get(&["foo"], fetcher.fetch(path_values)).await,
            [Err(BuildError::Other("foo".into()))]
        );
        assert_eq!(fetcher.requests(), [["foo"]]);

        table.invalidate("foo").await;

        assert_eq!(
            table.get(&["foo"], fetcher.fetch(path_values)).await,
            [Ok("foo".into())]
        );
        assert_eq!(fetcher.requests(), [["foo"], ["foo"]]);
    }

    #[tokio::test]
    async fn share_in_flight_lookup() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();
        let (sender, receiver) = oneshot::channel::<()>();
        let mut first = pin!(table.get(&["foo"], fetcher.fetch_after(receiver, path_values)));

        assert!(poll!(first.as_mut()).is_pending());

        let mut second = pin!(table.get(&["foo"], fetcher.fetch(path_errors)));

        assert!(poll!(second.as_mut()).is_pending());

        sender.send(()).unwrap();

        assert_eq!(
            wait(join(first, second)).await,
            (vec![Ok("foo".into())], vec![Ok("foo".into())])
        );
        assert_eq!(fetcher.requests(), [["foo"]]);
    }

    #[tokio::test]
    async fn wait_once_per_batch() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();
        let (sender, receiver) = oneshot::channel::<()>();
        let paths = (0..100).map(|index| index.to_string()).collect::<Vec<_>>();
        let paths = paths.iter().map(String::as_str).collect::<Vec<_>>();
        let mut first = pin!(table.get(&paths, fetcher.fetch_after(receiver, path_values)));

        assert!(poll!(first.as_mut()).is_pending());

        let mut second = pin!(table.get(&paths, fetcher.fetch(path_errors)));

        assert!(poll!(second.as_mut()).is_pending());

        sender.send(()).unwrap();

        let results = paths
            .iter()
            .map(|path| Ok(path.to_string()))
            .collect::<Vec<_>>();

        assert_eq!(wait(join(first, second)).await, (results.clone(), results));
        assert_eq!(fetcher.requests(), [paths.as_slice()]);
    }

    #[test]
    fn deduplicate_pending_batches() {
        let first = Arc::new(Batch::<()>::new());
        let second = Arc::new(Batch::new());
        let lookups = [
            Lookup::Pending(first.clone(), 0),
            Lookup::Ready(Ok(())),
            Lookup::Pending(second.clone(), 0),
            Lookup::Pending(first.clone(), 1),
            Lookup::Pending(second.clone(), 1),
        ];

        assert_eq!(
            pending_batches(&lookups)
                .map(Arc::as_ptr)
                .collect::<Vec<_>>(),
            [Arc::as_ptr(&first), Arc::as_ptr(&second)]
        );
    }

    #[tokio::test]
    async fn complete_lookup_after_dropping_requester() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();
        let (sender, receiver) = oneshot::channel::<()>();

        {
            let mut future = pin!(table.get(&["foo"], fetcher.fetch_after(receiver, path_values)));

            assert!(poll!(future.as_mut()).is_pending());
        }

        sender.send(()).unwrap();

        assert_eq!(
            wait(table.get(&["foo"], fetcher.fetch(path_errors))).await,
            [Ok("foo".into())]
        );
        assert_eq!(fetcher.requests(), [["foo"]]);
    }

    #[tokio::test]
    async fn fail_all_paths_on_batch_error() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();
        let error = BuildError::Other("foo".into());

        table.get(&["foo"], fetcher.fetch(path_values)).await;

        assert_eq!(
            table
                .get(
                    &["bar", "foo", "baz"],
                    fetcher.fetch({
                        let error = error.clone();
                        |_| Err(error)
                    })
                )
                .await,
            [Err(error.clone()), Ok("foo".into()), Err(error.clone())]
        );
        assert_eq!(
            table.get(&["baz"], fetcher.fetch(path_values)).await,
            [Err(error)]
        );
        assert_eq!(fetcher.requests(), [vec!["foo"], vec!["bar", "baz"]]);
    }

    #[tokio::test]
    async fn fail_on_mismatched_result_count() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();
        let error = BuildError::Other("file system returned 1 results for 2 paths".into());

        assert_eq!(
            table
                .get(
                    &["foo", "bar"],
                    fetcher.fetch(|paths| path_values(&paths[..1]))
                )
                .await,
            [Err(error.clone()), Err(error)]
        );
    }

    #[tokio::test]
    async fn complete_lookup_on_panicking_fetch() {
        let table = CacheTable::<String>::new();
        let fetcher = Fetcher::default();
        let (sender, receiver) = oneshot::channel::<()>();
        let mut first = pin!(table.get(
            &["foo"],
            fetcher.fetch_after(receiver, |_| panic!("fetch panicked"))
        ));

        assert!(poll!(first.as_mut()).is_pending());

        let mut second = pin!(table.get(&["foo"], fetcher.fetch(path_values)));

        assert!(poll!(second.as_mut()).is_pending());

        sender.send(()).unwrap();

        assert_eq!(
            wait(join(first, second)).await,
            (vec![aborted()], vec![aborted()])
        );
        assert_eq!(fetcher.requests(), [["foo"]]);
    }

    #[test]
    fn complete_lookup_on_dropped_fetch() {
        let table = CacheTable::<String>::new();
        let fetcher = Fetcher::default();
        let runtime = Builder::new_current_thread().build().unwrap();

        runtime.block_on(async {
            let mut future =
                pin!(table.get(&["foo"], fetcher.fetch_after(pending::<()>(), path_values)));

            assert!(poll!(future.as_mut()).is_pending());
        });
        drop(runtime);

        assert_eq!(
            table
                .get(&["foo"], fetcher.fetch(path_values))
                .now_or_never(),
            Some(vec![aborted()])
        );
        assert_eq!(fetcher.requests(), [["foo"]]);
    }

    #[tokio::test]
    async fn invalidate_entry() {
        let table = CacheTable::new();
        let fetcher = Fetcher::default();

        table.get(&["foo", "bar"], fetcher.fetch(path_values)).await;
        table.invalidate("foo").await;

        assert_eq!(
            table.get(&["foo", "bar"], fetcher.fetch(path_values)).await,
            [Ok("foo".into()), Ok("bar".into())]
        );
        assert_eq!(fetcher.requests(), [vec!["foo", "bar"], vec!["foo"]]);
    }

    #[tokio::test]
    async fn invalidate_file() {
        let cache = FileCache::new();
        let fetcher = Fetcher::default();

        cache
            .modified_times()
            .get(&["foo"], fetcher.fetch(|_| Ok(vec![Ok(None)])))
            .await;
        cache
            .content_hashes()
            .get(&["foo"], fetcher.fetch(|_| Ok(vec![Ok(None)])))
            .await;
        cache.invalidate("foo").await;

        assert_eq!(
            cache
                .modified_times()
                .get(
                    &["foo"],
                    fetcher.fetch(|_| Ok(vec![Ok(Some(SystemTime::UNIX_EPOCH))]))
                )
                .await,
            [Ok(Some(SystemTime::UNIX_EPOCH))]
        );
        assert_eq!(
            cache
                .content_hashes()
                .get(&["foo"], fetcher.fetch(|_| Ok(vec![Ok(Some(42))])))
                .await,
            [Ok(Some(42))]
        );
        assert_eq!(fetcher.requests(), [["foo"]; 4]);
    }

    #[tokio::test]
    async fn keep_newer_entry_after_stale_fetch() {
        let table = CacheTable::<String>::new();
        let fetcher = Fetcher::default();
        let (sender, receiver) = oneshot::channel::<()>();
        let mut stale = pin!(table.get(
            &["foo"],
            fetcher.fetch_after(receiver, |_| Ok(vec![Ok("old".into())]))
        ));

        assert!(poll!(stale.as_mut()).is_pending());

        table.invalidate("foo").await;

        assert_eq!(
            table
                .get(&["foo"], fetcher.fetch(|_| Ok(vec![Ok("new".into())])))
                .await,
            [Ok("new".into())]
        );

        sender.send(()).unwrap();

        assert_eq!(wait(stale).await, [Ok("old".into())]);
        assert_eq!(
            table.get(&["foo"], fetcher.fetch(path_values)).await,
            [Ok("new".into())]
        );
        assert_eq!(fetcher.requests(), [["foo"], ["foo"]]);
    }

    #[tokio::test]
    async fn fetch_again_after_stale_fetch() {
        let table = CacheTable::<String>::new();
        let fetcher = Fetcher::default();
        let (sender, receiver) = oneshot::channel::<()>();
        let mut stale = pin!(table.get(
            &["foo"],
            fetcher.fetch_after(receiver, |_| Ok(vec![Ok("old".into())]))
        ));

        assert!(poll!(stale.as_mut()).is_pending());

        table.invalidate("foo").await;
        sender.send(()).unwrap();

        assert_eq!(wait(stale).await, [Ok("old".into())]);
        assert_eq!(
            table
                .get(&["foo"], fetcher.fetch(|_| Ok(vec![Ok("new".into())])))
                .await,
            [Ok("new".into())]
        );
        assert_eq!(fetcher.requests(), [["foo"], ["foo"]]);
    }
}
