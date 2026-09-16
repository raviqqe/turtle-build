use crate::infrastructure::CommandRunner;
use alloc::sync::Arc;
use async_trait::async_trait;
use core::{
    error::Error,
    sync::atomic::{AtomicUsize, Ordering},
};
use std::{
    collections::HashMap,
    process::{ExitStatus, Output},
    sync::Mutex,
};
use tokio::task::yield_now;

#[derive(Clone, Debug, Default)]
pub struct FakeCommandRunner {
    commands: Arc<Mutex<Vec<String>>>,
    console_commands: Arc<Mutex<Vec<String>>>,
    outputs: HashMap<String, Output>,
    concurrency: Arc<AtomicUsize>,
    max_concurrency: Arc<AtomicUsize>,
}

impl FakeCommandRunner {
    pub fn new(outputs: HashMap<String, Output>) -> Self {
        Self {
            outputs,
            ..Default::default()
        }
    }

    pub fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }

    pub fn console_commands(&self) -> Vec<String> {
        self.console_commands.lock().unwrap().clone()
    }

    pub fn max_concurrency(&self) -> usize {
        self.max_concurrency.load(Ordering::SeqCst)
    }

    async fn execute(&self, command: &str) -> Output {
        let concurrency = self.concurrency.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_concurrency
            .fetch_max(concurrency, Ordering::SeqCst);

        // Suspend a command to let other commands run concurrently.
        yield_now().await;

        self.concurrency.fetch_sub(1, Ordering::SeqCst);

        self.outputs
            .get(command)
            .cloned()
            .unwrap_or_else(|| Output {
                status: ExitStatus::default(),
                stdout: vec![],
                stderr: vec![],
            })
    }
}

#[async_trait]
impl CommandRunner for FakeCommandRunner {
    async fn run(&self, command: &str) -> Result<Output, Box<dyn Error>> {
        self.commands.lock().unwrap().push(command.into());

        Ok(self.execute(command).await)
    }

    async fn run_with_console(&self, command: &str) -> Result<ExitStatus, Box<dyn Error>> {
        self.console_commands.lock().unwrap().push(command.into());

        Ok(self.execute(command).await.status)
    }
}
