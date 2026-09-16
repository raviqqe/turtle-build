use super::options::RunOptions;
use crate::{
    BuildError,
    build_graph::BuildGraph,
    context::Context,
    ir::{BuildId, Config, Pool},
};
use alloc::sync::Arc;
use core::pin::Pin;
use futures::future::Shared;
use scc::HashMap;
use tokio::sync::{Mutex, Semaphore, SemaphorePermit};

type BuildFuture = Shared<Pin<Box<dyn Future<Output = Result<(), BuildError>> + Send>>>;

pub struct RunContext {
    application: Arc<Context>,
    config: Arc<Config>,
    build_futures: HashMap<BuildId, BuildFuture>,
    build_graph: Mutex<BuildGraph>,
    pools: std::collections::HashMap<Arc<str>, Semaphore>,
    options: RunOptions,
}

impl RunContext {
    pub fn new(
        application: Arc<Context>,
        config: Arc<Config>,
        build_graph: BuildGraph,
        options: RunOptions,
    ) -> Self {
        Self {
            application,
            build_graph: build_graph.into(),
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

    pub fn application(&self) -> &Context {
        &self.application
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    // TODO Rename this `futures`.
    pub fn build_futures(&self) -> &HashMap<BuildId, BuildFuture> {
        &self.build_futures
    }

    // TODO Rename this `graph`.
    pub const fn build_graph(&self) -> &Mutex<BuildGraph> {
        &self.build_graph
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
