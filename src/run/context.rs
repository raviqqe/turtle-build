use super::options::RunOptions;
use crate::{
    BuildError,
    build_graph::BuildGraph,
    context::Context,
    ir::{BuildId, Config},
};
use alloc::sync::Arc;
use core::pin::Pin;
use futures::future::Shared;
use scc::HashMap;
use tokio::sync::{Mutex, Semaphore};

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

    pub fn build_futures(&self) -> &HashMap<BuildId, BuildFuture> {
        &self.build_futures
    }

    pub const fn build_graph(&self) -> &Mutex<BuildGraph> {
        &self.build_graph
    }

    pub const fn pools(&self) -> &std::collections::HashMap<Arc<str>, Semaphore> {
        &self.pools
    }

    pub const fn options(&self) -> &RunOptions {
        &self.options
    }
}
