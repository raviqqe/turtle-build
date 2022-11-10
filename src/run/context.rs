use super::{build_database::BuildDatabase, options::Options, BuildFuture};
use crate::context::Context as ApplicationContext;
use crate::{
    console::Console,
    ir::{BuildId, Configuration},
    validation::BuildGraph,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, RwLock, Semaphore};

#[derive(Debug)]
pub struct Context<'a> {
    application: Arc<ApplicationContext>,
    configuration: Arc<Configuration<'a>>,
    // TODO Use a concurrent hash map. We only need atomic insertion but not a great lock.
    build_futures: RwLock<HashMap<BuildId, BuildFuture<'a>>>,
    build_graph: Mutex<BuildGraph>,
    database: BuildDatabase,
    job_semaphore: Semaphore,
    console: Arc<Mutex<Console>>,
    options: Options,
}

impl<'a> Context<'a> {
    pub fn new(
        application: Arc<ApplicationContext>,
        configuration: Arc<Configuration<'a>>,
        build_graph: BuildGraph,
        database: BuildDatabase,
        job_semaphore: Semaphore,
        console: Arc<Mutex<Console>>,
        options: Options,
    ) -> Self {
        Self {
            application,
            build_graph: build_graph.into(),
            configuration,
            build_futures: RwLock::new(HashMap::new()),
            database,
            job_semaphore,
            console,
            options,
        }
    }

    pub fn application(&self) -> &ApplicationContext {
        &self.application
    }

    pub fn configuration(&self) -> &Configuration<'a> {
        &self.configuration
    }

    pub fn build_futures(&self) -> &RwLock<HashMap<BuildId, BuildFuture<'a>>> {
        &self.build_futures
    }

    pub fn build_graph(&self) -> &Mutex<BuildGraph> {
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
