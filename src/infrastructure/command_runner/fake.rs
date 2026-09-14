use crate::infrastructure::CommandRunner;
use async_trait::async_trait;
use std::{
    collections::HashMap,
    error::Error,
    process::{ExitStatus, Output},
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Default)]
pub struct FakeCommandRunner {
    commands: Arc<Mutex<Vec<String>>>,
    outputs: HashMap<String, Output>,
}

impl FakeCommandRunner {
    pub fn new(outputs: HashMap<String, Output>) -> Self {
        Self {
            commands: Default::default(),
            outputs,
        }
    }

    pub fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }
}

#[async_trait]
impl CommandRunner for FakeCommandRunner {
    async fn run(&self, command: &str) -> Result<Output, Box<dyn Error>> {
        self.commands.lock().unwrap().push(command.into());

        Ok(self
            .outputs
            .get(command)
            .cloned()
            .unwrap_or_else(|| Output {
                status: ExitStatus::default(),
                stdout: vec![],
                stderr: vec![],
            }))
    }
}
