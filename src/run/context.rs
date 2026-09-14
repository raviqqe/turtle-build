use super::{BuildFuture, options::RunOptions};
use crate::{
    build_graph::BuildGraph,
    context::Context as ApplicationContext,
    ir::{BuildId, Config},
};
use alloc::sync::Arc;
use scc::HashMap;
use tokio::sync::Mutex;

pub struct Context {
    application: Arc<ApplicationContext>,
    config: Arc<Config>,
    build_futures: HashMap<BuildId, BuildFuture>,
    build_graph: Mutex<BuildGraph>,
    options: RunOptions,
}

impl Context {
    pub fn new(
        application: Arc<ApplicationContext>,
        config: Arc<Config>,
        build_graph: BuildGraph,
        options: RunOptions,
    ) -> Self {
        Self {
            application,
            build_graph: build_graph.into(),
            config,
            build_futures: HashMap::new(),
            options,
        }
    }

    pub fn application(&self) -> &ApplicationContext {
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

    pub const fn options(&self) -> &RunOptions {
        &self.options
    }
}
