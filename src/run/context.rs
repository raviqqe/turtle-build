use super::{build_database::BuildDatabase, options::Options, BuildFuture};
use crate::{console::Console, ir::Configuration, validation::BuildGraph};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, RwLock, Semaphore};

#[derive(Debug)]
pub struct Context<'a> {
    configuration: Arc<Configuration<'a>>,
    // TODO Use a concurrent hash map. We only need atomic insertion but not a great lock.
    build_futures: RwLock<HashMap<u64, BuildFuture>>,
    build_graph: Mutex<BuildGraph<'a>>,
    database: BuildDatabase,
    job_semaphore: Semaphore,
    console: Arc<Mutex<Console>>,
    options: Options,
}

impl<'a> Context<'a> {
    pub fn new(
        configuration: Arc<Configuration>,
        build_graph: BuildGraph,
        database: BuildDatabase,
        job_semaphore: Semaphore,
        console: Arc<Mutex<Console>>,
        options: Options,
    ) -> Self {
        Self {
            build_graph: build_graph.into(),
            configuration,
            build_futures: RwLock::new(HashMap::new()),
            database,
            job_semaphore,
            console,
            options,
        }
    }

    pub fn configuration(&self) -> &Configuration {
        &self.configuration
    }

    pub fn build_futures(&self) -> &RwLock<HashMap<u64, BuildFuture>> {
        &self.build_futures
    }

    pub fn build_graph(&self) -> &Mutex<BuildGraph<'a>> {
        &self.build_graph
    }

    pub fn database(&self) -> &BuildDatabase {
        &self.database
    }

    pub fn job_semaphore(&self) -> &Semaphore {
        &self.job_semaphore
    }

    pub fn console(&self) -> &Mutex<Console> {
        &self.console
    }

    pub fn options(&self) -> &Options {
        &self.options
    }
}
