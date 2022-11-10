use std::fmt::Debug;

use crate::infrastructure::FileSystem;

#[derive(Debug)]
pub struct Context {
    file_system: Box<dyn FileSystem + Send + Sync + 'static>,
}

impl Context {
    pub fn new(file_system: impl FileSystem + Send + Sync + 'static) -> Self {
        Self {
            file_system: Box::new(file_system),
        }
    }

    pub fn file_system(&self) -> &(dyn FileSystem + Send + Sync) {
        &*self.file_system
    }
}
