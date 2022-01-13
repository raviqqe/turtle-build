use super::{build_database::BuildDatabase, BuildFuture};
use crate::ir::Configuration;
use std::collections::HashMap;
use tokio::sync::{RwLock, Semaphore};

#[derive(Debug)]
pub struct Context {
    configuration: Configuration,
    // TODO Use a concurrent hash map. We only need atomic insertion but not a great lock.
    builds: RwLock<HashMap<String, BuildFuture>>,
    database: BuildDatabase,
    job_semaphore: Semaphore,
}

impl Context {
    pub fn new(
        configuration: Configuration,
        database: BuildDatabase,
        job_semaphore: Semaphore,
    ) -> Self {
        Self {
            configuration,
            builds: RwLock::new(HashMap::new()),
            database,
            job_semaphore,
        }
    }

    pub fn configuration(&self) -> &Configuration {
        &self.configuration
    }

    pub fn builds(&self) -> &RwLock<HashMap<String, BuildFuture>> {
        &self.builds
    }

    pub fn database(&self) -> &BuildDatabase {
        &self.database
    }

    pub fn job_semaphore(&self) -> &Semaphore {
        &self.job_semaphore
    }
}
