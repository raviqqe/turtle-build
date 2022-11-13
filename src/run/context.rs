use super::{options::Options, BuildFuture};
use crate::{
    build_graph::BuildGraph,
    context::Context as ApplicationContext,
    ir::{BuildId, Configuration},
};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Context {
    application: Arc<ApplicationContext>,
    configuration: Arc<Configuration>,
    build_futures: DashMap<BuildId, BuildFuture>,
    build_graph: Mutex<BuildGraph>,
    options: Options,
}

impl Context {
    pub fn new(
        application: Arc<ApplicationContext>,
        configuration: Arc<Configuration>,
        build_graph: BuildGraph,
        options: Options,
    ) -> Self {
        Self {
            application,
            build_graph: build_graph.into(),
            configuration,
            build_futures: DashMap::new(),
            options,
        }
    }

    pub fn application(&self) -> &ApplicationContext {
        &self.application
    }

    pub fn configuration(&self) -> &Configuration {
        &self.configuration
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
