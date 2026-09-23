use super::{file_cache::FileCache, job_queue::JobQueue, options::RunOptions};
use crate::{
    BuildError,
    build_graph::BuildGraph,
    context::Context,
    ir::{BuildId, Config, DynamicConfig, Pool},
};
use alloc::sync::Arc;
use core::{
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
};
use futures::future::Shared;
use scc::HashMap;
use tokio::sync::{Mutex, OnceCell, Semaphore, SemaphorePermit};

type BuildFuture = Shared<Pin<Box<dyn Future<Output = Result<(), BuildError>> + Send>>>;

pub struct RunContext {
    build: Arc<Context>,
    config: Arc<Config>,
    build_futures: HashMap<BuildId, BuildFuture>,
    build_graph: Mutex<BuildGraph>,
    dynamic_configs: std::collections::HashMap<Arc<str>, OnceCell<DynamicConfig>>,
    file_cache: FileCache,
    header_dependencies: std::collections::HashMap<BuildId, Vec<Arc<str>>>,
    job_queue: JobQueue,
    pools: std::collections::HashMap<Arc<str>, Semaphore>,
    priorities: std::collections::HashMap<BuildId, usize>,
    dynamic_priority: AtomicUsize,
    options: RunOptions,
}

impl RunContext {
    pub fn new(
        build: Arc<Context>,
        config: Arc<Config>,
        build_graph: BuildGraph,
        header_dependencies: std::collections::HashMap<BuildId, Vec<Arc<str>>>,
        priorities: std::collections::HashMap<BuildId, usize>,
        options: RunOptions,
    ) -> Self {
        Self {
            file_cache: FileCache::new(build.file_system().clone()),
            build,
            build_graph: build_graph.into(),
            dynamic_configs: config
                .outputs()
                .values()
                .filter_map(|build| Some((build.dynamic_module()?.clone(), Default::default())))
                .collect(),
            header_dependencies,
            job_queue: JobQueue::new(options.job_limit),
            dynamic_priority: AtomicUsize::new(priorities.len()),
            priorities,
            pools: config
                .pools()
                .iter()
                .map(|(name, depth)| {
                    (
                        name.clone(),
                        Semaphore::new(depth.get().min(Semaphore::MAX_PERMITS)),
                    )
                })
                .collect(),
            config,
            build_futures: HashMap::new(),
            options,
        }
    }

    pub fn build(&self) -> &Context {
        &self.build
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub const fn build_futures(&self) -> &HashMap<BuildId, BuildFuture> {
        &self.build_futures
    }

    pub const fn build_graph(&self) -> &Mutex<BuildGraph> {
        &self.build_graph
    }

    pub fn dynamic_config(&self, path: &str) -> &OnceCell<DynamicConfig> {
        &self.dynamic_configs[path]
    }

    pub const fn file_cache(&self) -> &FileCache {
        &self.file_cache
    }

    pub fn header_dependencies(&self, id: BuildId) -> &[Arc<str>] {
        self.header_dependencies
            .get(&id)
            .map_or_default(Vec::as_slice)
    }

    pub const fn job_queue(&self) -> &JobQueue {
        &self.job_queue
    }

    pub fn priority(&self, id: BuildId) -> usize {
        self.priorities
            .get(&id)
            .copied()
            .unwrap_or_else(|| self.dynamic_priority.fetch_add(1, Ordering::Relaxed))
    }

    pub async fn pool(
        &self,
        pool: Option<&Pool>,
    ) -> Result<Option<SemaphorePermit<'_>>, BuildError> {
        let Some(Pool::Limited(name)) = pool else {
            return Ok(None);
        };

        Ok(Some(self.pools[name].acquire().await?))
    }

    pub const fn options(&self) -> &RunOptions {
        &self.options
    }
}
