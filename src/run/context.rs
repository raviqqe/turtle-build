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
use rapidhash::{RapidHashMap, fast::RandomState};
use scc::HashIndex;
use tokio::sync::{Mutex, OnceCell, Semaphore, SemaphorePermit};

type BuildFuture = Shared<Pin<Box<dyn Future<Output = Result<bool, BuildError>> + Send>>>;

pub struct RunContext {
    build: Arc<Context>,
    config: Arc<Config>,
    build_futures: HashIndex<BuildId, BuildFuture, RandomState>,
    build_graph: Mutex<BuildGraph>,
    dynamic_configs: RapidHashMap<Arc<str>, OnceCell<DynamicConfig>>,
    file_cache: FileCache,
    header_inputs: RapidHashMap<BuildId, Vec<Arc<str>>>,
    pools: RapidHashMap<Arc<str>, Semaphore>,
    options: RunOptions,
}

impl RunContext {
    pub fn new(
        build: Arc<Context>,
        config: Arc<Config>,
        build_graph: BuildGraph,
        header_inputs: RapidHashMap<BuildId, Vec<Arc<str>>>,
        options: RunOptions,
    ) -> Self {
        Self {
            file_cache: FileCache::new(build.file_system().clone(), config.outputs().len()),
            build,
            build_graph: build_graph.into(),
            dynamic_configs: config
                .outputs()
                .values()
                .filter_map(|build| Some((build.dynamic_module()?.clone(), Default::default())))
                .collect(),
            header_inputs,
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
            build_futures: HashIndex::with_capacity_and_hasher(
                config.outputs().len(),
                Default::default(),
            ),
            config,
            options,
        }
    }

    pub fn build(&self) -> &Context {
        &self.build
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub const fn build_futures(&self) -> &HashIndex<BuildId, BuildFuture, RandomState> {
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

    pub fn header_inputs(&self, id: BuildId) -> &[Arc<str>] {
        self.header_inputs.get(&id).map_or_default(Vec::as_slice)
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
