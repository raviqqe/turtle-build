use crate::infrastructure::{CommandRunner, Console, Database, FileSystem};
use tokio::sync::Mutex;

pub struct Context {
    command_runner: Box<dyn CommandRunner + Send + Sync + 'static>,
    console: Mutex<Box<dyn Console + Send + Sync + 'static>>,
    database: Box<dyn Database + Send + Sync + 'static>,
    file_system: Box<dyn FileSystem + Send + Sync + 'static>,
}

impl Context {
    pub fn new(
        command_runner: impl CommandRunner + Send + Sync + 'static,
        console: impl Console + Send + Sync + 'static,
        database: impl Database + Send + Sync + 'static,
        file_system: impl FileSystem + Send + Sync + 'static,
    ) -> Self {
        Self {
            command_runner: Box::new(command_runner),
            console: Mutex::new(Box::new(console)),
            file_system: Box::new(file_system),
            database: Box::new(database),
        }
    }

    pub fn command_runner(&self) -> &(dyn CommandRunner + Send + Sync) {
        &*self.command_runner
    }

    pub fn console(&self) -> &Mutex<Box<dyn Console + Send + Sync>> {
        &self.console
    }

    pub fn database(&self) -> &(dyn Database + Send + Sync) {
        &*self.database
    }

    pub fn file_system(&self) -> &(dyn FileSystem + Send + Sync) {
        &*self.file_system
    }
}
