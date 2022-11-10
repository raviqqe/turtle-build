use crate::infrastructure::{Console, Database, FileSystem};
use std::sync::RwLock;
use tokio::sync::Mutex;

pub struct Context {
    console: Mutex<Box<dyn Console + Send + Sync + 'static>>,
    file_system: Box<dyn FileSystem + Send + Sync + 'static>,
    database: RwLock<Box<dyn Database + Send + Sync + 'static>>,
}

impl Context {
    pub fn new(
        console: impl Console + Send + Sync + 'static,
        file_system: impl FileSystem + Send + Sync + 'static,
        database: impl Database + Send + Sync + 'static,
    ) -> Self {
        Self {
            console: Mutex::new(Box::new(console)),
            file_system: Box::new(file_system),
            database: RwLock::new(Box::new(database)),
        }
    }

    pub fn console(&self) -> &Mutex<Box<dyn Console + Send + Sync>> {
        &self.console
    }

    pub fn file_system(&self) -> &(dyn FileSystem + Send + Sync) {
        &*self.file_system
    }

    pub fn database(&self) -> &RwLock<Box<dyn Database + Send + Sync>> {
        &self.database
    }
}
