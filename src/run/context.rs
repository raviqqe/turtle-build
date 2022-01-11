use super::build_database::BuildDatabase;
use tokio::sync::Semaphore;

#[derive(Debug)]
pub struct Context {
    database: BuildDatabase,
    job_semaphore: Semaphore,
}

impl Context {
    pub fn new(database: BuildDatabase, job_semaphore: Semaphore) -> Self {
        Self {
            database,
            job_semaphore,
        }
    }

    pub fn database(&self) -> &BuildDatabase {
        &self.database
    }

    pub fn job_semaphore(&self) -> &Semaphore {
        &self.job_semaphore
    }
}
