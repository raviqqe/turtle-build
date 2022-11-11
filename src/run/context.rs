use super::{options::Options, BuildFuture};
use crate::{
    context::Context as ApplicationContext,
    ir::{BuildId, Configuration},
    validation::BuildGraph,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, RwLock, Semaphore};

pub struct Context {
    application: Arc<ApplicationContext>,
    configuration: Arc<Configuration>,
    // TODO Use a concurrent hash map. We only need atomic insertion but not a great lock.
    build_futures: RwLock<HashMap<BuildId, BuildFuture>>,
    build_graph: Mutex<BuildGraph>,
    job_semaphore: Semaphore,
    options: Options,
}

impl Context {
    pub fn new(
        application: Arc<ApplicationContext>,
        configuration: Arc<Configuration>,
        build_graph: BuildGraph,
        job_semaphore: Semaphore,
        options: Options,
    ) -> Self {
        Self {
            application,
            build_graph: build_graph.into(),
            configuration,
            build_futures: RwLock::new(HashMap::new()),
            job_semaphore,
            options,
        }
    }

    pub fn application(&self) -> &ApplicationContext {
        &self.application
    }

    pub fn configuration(&self) -> &Configuration {
        &self.configuration
    }

    pub fn build_futures(&self) -> &RwLock<HashMap<BuildId, BuildFuture>> {
        &self.build_futures
    }

    pub fn build_graph(&self) -> &Mutex<BuildGraph> {
        &self.build_graph
    }

    pub fn job_semaphore(&self) -> &Semaphore {
        &self.job_semaphore
    }

    pub fn options(&self) -> &Options {
        &self.options
    }
}
