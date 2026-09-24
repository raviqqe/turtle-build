use crate::infrastructure::{Console, ConsoleBuffer, ConsoleError};
use alloc::sync::Arc;
use core::mem::take;
use std::sync::PoisonError;
use tokio::sync::{Mutex, MutexGuard};

/// A queue of outputs to a console shared by builds.
pub struct ConsoleQueue {
    console: Arc<Mutex<dyn Console + Send + Sync>>,
    pool: Mutex<()>,
    state: std::sync::Mutex<State>,
}

#[derive(Default)]
struct State {
    locked: bool,
    pending: ConsoleBuffer,
}

impl ConsoleQueue {
    pub fn new(console: Arc<Mutex<dyn Console + Send + Sync>>) -> Self {
        Self {
            console,
            pool: Mutex::new(()),
            state: Default::default(),
        }
    }

    /// Writes a buffer to the console after queued ones, or queues it while
    /// the console is locked for one build.
    pub async fn write(&self, buffer: ConsoleBuffer) -> Result<(), ConsoleError> {
        let mut console = self.console.lock().await;
        let buffer = {
            let mut state = self.state();

            state.pending.append(buffer);

            if state.locked {
                return Ok(());
            }

            take(&mut state.pending)
        };

        buffer.write_to(&mut *console).await
    }

    /// Locks the console for one build after writing queued buffers.
    pub async fn lock(&self) -> Result<ConsoleLock<'_>, ConsoleError> {
        let pool = self.pool.lock().await;
        let mut console = self.console.lock().await;
        let pending = {
            let mut state = self.state();

            state.locked = true;

            take(&mut state.pending)
        };

        pending.write_to(&mut *console).await?;

        Ok(ConsoleLock {
            queue: self,
            _pool: pool,
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A console locked for one build.
pub struct ConsoleLock<'a> {
    queue: &'a ConsoleQueue,
    _pool: MutexGuard<'a, ()>,
}

impl ConsoleLock<'_> {
    pub async fn write(&self, buffer: ConsoleBuffer) -> Result<(), ConsoleError> {
        buffer.write_to(&mut *self.queue.console.lock().await).await
    }

    pub async fn flush(&self) -> Result<(), ConsoleError> {
        self.queue.console.lock().await.flush().await
    }
}

impl Drop for ConsoleLock<'_> {
    fn drop(&mut self) {
        self.queue.state().locked = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::FakeConsole;
    use core::pin::pin;
    use futures::poll;
    use pretty_assertions::assert_eq;

    fn create_queue(console: &FakeConsole) -> ConsoleQueue {
        ConsoleQueue::new(Arc::new(Mutex::new(console.clone())))
    }

    fn stderr_buffer(text: &str) -> ConsoleBuffer {
        let mut buffer = ConsoleBuffer::default();

        buffer.write_stderr(text.as_bytes());

        buffer
    }

    #[tokio::test]
    async fn write_buffer() {
        let console = FakeConsole::default();

        create_queue(&console)
            .write(stderr_buffer("foo"))
            .await
            .unwrap();

        assert_eq!(console.stderr(), "foo");
    }

    #[tokio::test]
    async fn write_buffer_with_lock() {
        let console = FakeConsole::default();
        let queue = create_queue(&console);

        queue
            .lock()
            .await
            .unwrap()
            .write(stderr_buffer("foo"))
            .await
            .unwrap();

        assert_eq!(console.stderr(), "foo");
    }

    #[tokio::test]
    async fn flush_console() {
        let console = FakeConsole::default();
        let queue = create_queue(&console);
        let lock = queue.lock().await.unwrap();

        lock.write(stderr_buffer("foo")).await.unwrap();
        lock.flush().await.unwrap();

        assert_eq!(console.flushed_stderr(), "foo");
    }

    #[tokio::test]
    async fn queue_buffer_while_locked() {
        let console = FakeConsole::default();
        let queue = create_queue(&console);
        let lock = queue.lock().await.unwrap();

        queue.write(stderr_buffer("foo")).await.unwrap();

        assert_eq!(console.stderr(), "");

        drop(lock);
        queue.write(stderr_buffer("bar")).await.unwrap();

        assert_eq!(console.stderr(), "foobar");
    }

    #[tokio::test]
    async fn write_queued_buffers_on_lock() {
        let console = FakeConsole::default();
        let queue = create_queue(&console);
        let lock = queue.lock().await.unwrap();

        queue.write(stderr_buffer("foo")).await.unwrap();
        drop(lock);

        let lock = queue.lock().await.unwrap();

        assert_eq!(console.stderr(), "foo");

        lock.write(stderr_buffer("bar")).await.unwrap();

        assert_eq!(console.stderr(), "foobar");
    }

    #[tokio::test]
    async fn wait_for_lock() {
        let queue = create_queue(&Default::default());
        let lock = queue.lock().await.unwrap();
        let mut future = pin!(queue.lock());

        assert!(poll!(&mut future).is_pending());

        drop(lock);

        assert!(poll!(&mut future).is_ready());
    }
}
