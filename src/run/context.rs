use tokio::sync::Semaphore;

#[derive(Debug)]
pub struct Context {
    job_semaphore: Semaphore,
}

impl Context {
    pub fn new(job_limit: usize) -> Self {
        Self {
            job_semaphore: Semaphore::new(job_limit),
        }
    }

    pub fn job_semaphore(&self) -> &Semaphore {
        &self.job_semaphore
    }
}
