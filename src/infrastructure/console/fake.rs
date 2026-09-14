use crate::infrastructure::Console;
use async_trait::async_trait;
use alloc::sync::Arc;
use core::error::Error;
use std::sync::Mutex;

#[derive(Clone, Debug, Default)]
pub struct FakeConsole {
    stdout: Arc<Mutex<Vec<u8>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
}

impl FakeConsole {
    pub fn stdout(&self) -> String {
        String::from_utf8(self.stdout.lock().unwrap().clone()).unwrap()
    }

    pub fn stderr(&self) -> String {
        String::from_utf8(self.stderr.lock().unwrap().clone()).unwrap()
    }
}

#[async_trait]
impl Console for FakeConsole {
    async fn write_stdout(&mut self, buffer: &[u8]) -> Result<(), Box<dyn Error>> {
        self.stdout.lock().unwrap().extend_from_slice(buffer);

        Ok(())
    }

    async fn write_stderr(&mut self, buffer: &[u8]) -> Result<(), Box<dyn Error>> {
        self.stderr.lock().unwrap().extend_from_slice(buffer);

        Ok(())
    }
}
