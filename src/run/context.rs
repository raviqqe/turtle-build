use super::{file_cache::FileCache, options::RunOptions};
use crate::{
    BuildError,
    build_graph::BuildGraph,
    context::Context,
    ir::{BuildId, Config, DynamicConfig, Pool},
};
use alloc::sync::Arc;
use core::pin::Pin;
use futures::future::Shared;
use moka::future::Cache;
use scc::HashMap;
use tokio::sync::{Mutex, Semaphore, SemaphorePermit};

type BuildFuture = Shared<Pin<Box<dyn Future<Output = Result<(), BuildError>> + Send>>>;

pub struct RunContext {
    build: Arc<Context>,
    config: Arc<Config>,
    build_futures: HashMap<BuildId, BuildFuture>,
    build_graph: Mutex<BuildGraph>,
    dynamic_configs: Cache<Arc<str>, Arc<DynamicConfig>>,
    file_cache: FileCache,
    header_dependencies: std::collections::HashMap<BuildId, Vec<Arc<str>>>,
    pools: std::collections::HashMap<Arc<str>, Semaphore>,
    options: RunOptions,
}

impl RunContext {
    pub fn new(
        build: Arc<Context>,
        config: Arc<Config>,
        build_graph: BuildGraph,
        header_dependencies: std::collections::HashMap<BuildId, Vec<Arc<str>>>,
        options: RunOptions,
    ) -> Self {
        Self {
            file_cache: FileCache::new(build.file_system().clone()),
            build,
            build_graph: build_graph.into(),
            dynamic_configs: Cache::builder().build(),
            header_dependencies,
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

    pub const fn dynamic_configs(&self) -> &Cache<Arc<str>, Arc<DynamicConfig>> {
        &self.dynamic_configs
    }

    pub const fn file_cache(&self) -> &FileCache {
        &self.file_cache
    }

    pub fn header_dependencies(&self, id: BuildId) -> &[Arc<str>] {
        self.header_dependencies
            .get(&id)
            .map_or_default(Vec::as_slice)
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
