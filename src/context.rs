use crate::infrastructure::{Console, FileSystem};
use std::fmt::Debug;
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct Context {
    console: Mutex<Box<dyn Console + Send + Sync + 'static>>,
    file_system: Box<dyn FileSystem + Send + Sync + 'static>,
}

impl Context {
    pub fn new(
        console: impl Console + Send + Sync + 'static,
        file_system: impl FileSystem + Send + Sync + 'static,
    ) -> Self {
        Self {
            console: Mutex::new(Box::new(console)),
            file_system: Box::new(file_system),
        }
    }

    pub fn console(&self) -> &Mutex<Box<dyn Console + Send + Sync>> {
        &self.console
    }

    pub fn file_system(&self) -> &(dyn FileSystem + Send + Sync) {
        &*self.file_system
    }
}
