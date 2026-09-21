use crate::infrastructure::{CommandRunner, Console, Database, FileSystem};
use alloc::sync::Arc;
use tokio::sync::Mutex;

/// A context.
pub struct Context {
    command_runner: Box<dyn CommandRunner + Send + Sync>,
    console: Arc<Mutex<dyn Console + Send + Sync>>,
    database: Box<dyn Database + Send + Sync>,
    file_system: Arc<dyn FileSystem + Send + Sync>,
}

impl Context {
    /// Creates a context.
    pub fn new(
        command_runner: impl CommandRunner + Send + Sync + 'static,
        console: Arc<Mutex<impl Console + Send + Sync + 'static>>,
        database: impl Database + Send + Sync + 'static,
        file_system: impl FileSystem + Send + Sync + 'static,
    ) -> Self {
        Self {
            command_runner: Box::new(command_runner),
            console,
            file_system: Arc::new(file_system),
            database: Box::new(database),
        }
    }

    /// Returns a command runner.
    pub fn command_runner(&self) -> &(dyn CommandRunner + Send + Sync) {
        &*self.command_runner
    }

    /// Returns a console.
    pub fn console(&self) -> &Mutex<dyn Console + Send + Sync> {
        &self.console
    }

    /// Returns a database.
    pub fn database(&self) -> &(dyn Database + Send + Sync) {
        &*self.database
    }

    /// Returns a file system.
    pub fn file_system(&self) -> &Arc<dyn FileSystem + Send + Sync> {
        &self.file_system
    }
}
