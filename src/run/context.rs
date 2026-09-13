use super::{BuildFuture, options::Options};
use crate::{
    build_graph::BuildGraph,
    context::Context as ApplicationContext,
    ir::{BuildId, Config},
};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Context {
    application: Arc<ApplicationContext>,
    config: Arc<Config>,
    build_futures: DashMap<BuildId, BuildFuture>,
    build_graph: Mutex<BuildGraph>,
    options: Options,
}

impl Context {
    pub fn new(
        application: Arc<ApplicationContext>,
        config: Arc<Config>,
        build_graph: BuildGraph,
        options: Options,
    ) -> Self {
        Self {
            application,
            build_graph: build_graph.into(),
            config,
            build_futures: DashMap::new(),
            options,
        }
    }

    pub fn application(&self) -> &ApplicationContext {
        &self.application
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn build_futures(&self) -> &DashMap<BuildId, BuildFuture> {
        &self.build_futures
    }

    pub fn build_graph(&self) -> &Mutex<BuildGraph> {
        &self.build_graph
    }

    pub fn options(&self) -> &Options {
        &self.options
    }
}
