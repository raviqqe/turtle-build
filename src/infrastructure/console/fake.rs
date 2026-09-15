use crate::infrastructure::Console;
use alloc::sync::Arc;
use async_trait::async_trait;
use core::error::Error;
use std::sync::Mutex;

#[derive(Clone, Debug, Default)]
pub struct FakeConsole {
    stdout: Arc<Mutex<Vec<u8>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    flushed_stderr: Arc<Mutex<Vec<u8>>>,
    failing: bool,
}

impl FakeConsole {
    pub fn failing() -> Self {
        Self {
            failing: true,
            ..Default::default()
        }
    }

    pub fn stdout(&self) -> String {
        String::from_utf8(self.stdout.lock().unwrap().clone()).unwrap()
    }

    pub fn stderr(&self) -> String {
        String::from_utf8(self.stderr.lock().unwrap().clone()).unwrap()
    }

    pub fn flushed_stderr(&self) -> String {
        String::from_utf8(self.flushed_stderr.lock().unwrap().clone()).unwrap()
    }

    fn check_failure(&self) -> Result<(), Box<dyn Error>> {
        if self.failing {
            Err("console failure".into())
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl Console for FakeConsole {
    async fn write_stdout(&mut self, buffer: &[u8]) -> Result<(), Box<dyn Error>> {
        self.check_failure()?;
        self.stdout.lock().unwrap().extend_from_slice(buffer);

        Ok(())
    }

    async fn write_stderr(&mut self, buffer: &[u8]) -> Result<(), Box<dyn Error>> {
        self.check_failure()?;
        self.stderr.lock().unwrap().extend_from_slice(buffer);

        Ok(())
    }

    async fn flush(&mut self) -> Result<(), Box<dyn Error>> {
        *self.flushed_stderr.lock().unwrap() = self.stderr.lock().unwrap().clone();

        Ok(())
    }
}
