use crate::infrastructure::FileSystem;

pub struct Context {
    file_system: Box<dyn FileSystem>,
}

impl Context {
    pub fn new(file_system: impl FileSystem + 'static) -> Self {
        Self {
            file_system: Box::new(file_system),
        }
    }

    pub fn file_system(&self) -> &dyn FileSystem {
        &*self.file_system
    }
}
