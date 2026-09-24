use super::buffer::ConsoleBuffer;
use crate::infrastructure::{Console, ConsoleError};
use alloc::sync::Arc;
use core::mem::take;
use std::sync::PoisonError;
use tokio::sync::{Mutex, MutexGuard};

/// A console multiplexed among concurrent writers.
///
/// Outputs queued while the console is locked are written when the lock
/// holder flushes the console or when it is locked or flushed next time.
pub struct MultiplexedConsole {
    console: Arc<Mutex<dyn Console + Send + Sync>>,
    pending: std::sync::Mutex<ConsoleBuffer>,
}

impl MultiplexedConsole {
    pub fn new(console: Arc<Mutex<dyn Console + Send + Sync>>) -> Self {
        Self {
            console,
            pending: Default::default(),
        }
    }

    /// Locks the console exclusively after writing pending outputs.
    pub async fn lock(&self) -> Result<ConsolePermit<'_>, ConsoleError> {
        let mut guard = self.console.lock().await;

        self.drain(&mut *guard).await?;

        Ok(ConsolePermit::Lock(ConsoleLock {
            console: self,
            guard,
        }))
    }

    /// Queues outputs to the console.
    pub fn queue(&self) -> ConsolePermit<'_> {
        ConsolePermit::Queue(ConsoleQueue {
            console: self,
            buffer: Default::default(),
        })
    }

    async fn drain(&self, console: &mut (dyn Console + Send + Sync)) -> Result<(), ConsoleError> {
        // Do not inline this to avoid holding a lock of pending outputs across an await point.
        let buffer = take(&mut *self.pending());

        buffer.write_to(console).await
    }

    async fn write_queued(&self, buffer: ConsoleBuffer) -> Result<(), ConsoleError> {
        let mut console = {
            let mut pending = self.pending();

            pending.append(buffer);

            let Ok(console) = self.console.try_lock() else {
                return Ok(());
            };

            console
        };

        loop {
            let buffer = {
                let mut pending = self.pending();

                if pending.is_empty() {
                    // Unlock the console while holding the pending outputs so that no
                    // output queued by others stays unwritten until the next flush.
                    drop(console);

                    return Ok(());
                }

                take(&mut *pending)
            };

            buffer.write_to(&mut *console).await?;
        }
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, ConsoleBuffer> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A permit to write to a multiplexed console.
pub enum ConsolePermit<'a> {
    /// A permit locking a console exclusively.
    Lock(ConsoleLock<'a>),
    /// A permit queuing outputs to a console.
    Queue(ConsoleQueue<'a>),
}

impl ConsolePermit<'_> {
    pub async fn write_stdout(&mut self, bytes: &[u8]) -> Result<(), ConsoleError> {
        match self {
            Self::Lock(lock) => lock.guard.write_stdout(bytes).await,
            Self::Queue(queue) => {
                queue.buffer.write_stdout(bytes);

                Ok(())
            }
        }
    }

    pub async fn write_stderr(&mut self, bytes: &[u8]) -> Result<(), ConsoleError> {
        match self {
            Self::Lock(lock) => lock.guard.write_stderr(bytes).await,
            Self::Queue(queue) => {
                queue.buffer.write_stderr(bytes);

                Ok(())
            }
        }
    }

    /// Flushes outputs.
    ///
    /// A lock writes pending outputs and flushes the console. A queue queues
    /// its outputs and writes pending ones unless the console is locked.
    pub async fn flush(&mut self) -> Result<(), ConsoleError> {
        match self {
            Self::Lock(lock) => {
                lock.console.drain(&mut *lock.guard).await?;
                lock.guard.flush().await
            }
            Self::Queue(queue) => queue.console.write_queued(take(&mut queue.buffer)).await,
        }
    }
}

/// An exclusive lock of a multiplexed console.
pub struct ConsoleLock<'a> {
    console: &'a MultiplexedConsole,
    guard: MutexGuard<'a, dyn Console + Send + Sync>,
}

/// A queue of outputs to a multiplexed console.
pub struct ConsoleQueue<'a> {
    console: &'a MultiplexedConsole,
    buffer: ConsoleBuffer,
}

impl Drop for ConsoleQueue<'_> {
    fn drop(&mut self) {
        self.console.pending().append(take(&mut self.buffer));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::FakeConsole;
    use core::pin::pin;
    use futures::poll;

    fn create_console(console: &FakeConsole) -> MultiplexedConsole {
        MultiplexedConsole::new(Arc::new(Mutex::new(console.clone())))
    }

    async fn queue_stderr(console: &MultiplexedConsole, text: &str) {
        let mut permit = console.queue();

        permit.write_stderr(text.as_bytes()).await.unwrap();
        permit.flush().await.unwrap();
    }

    mod lock {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn write_stdout() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);
            let mut permit = multiplexed.lock().await.unwrap();

            permit.write_stdout(b"foo").await.unwrap();

            assert_eq!(console.stdout(), "foo");
        }

        #[tokio::test]
        async fn write_stderr() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);
            let mut permit = multiplexed.lock().await.unwrap();

            permit.write_stderr(b"foo").await.unwrap();

            assert_eq!(console.stderr(), "foo");
        }

        #[tokio::test]
        async fn flush() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);
            let mut permit = multiplexed.lock().await.unwrap();

            permit.write_stderr(b"foo").await.unwrap();
            permit.flush().await.unwrap();

            assert_eq!(console.flushed_stderr(), "foo");
        }

        #[tokio::test]
        async fn write_pending_outputs_on_lock() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);
            let permit = multiplexed.lock().await.unwrap();

            queue_stderr(&multiplexed, "foo").await;
            drop(permit);

            let mut permit = multiplexed.lock().await.unwrap();

            assert_eq!(console.stderr(), "foo");

            permit.write_stderr(b"bar").await.unwrap();

            assert_eq!(console.stderr(), "foobar");
        }

        #[tokio::test]
        async fn write_pending_outputs_on_flush() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);
            let mut permit = multiplexed.lock().await.unwrap();

            permit.write_stderr(b"foo").await.unwrap();
            queue_stderr(&multiplexed, "bar").await;

            assert_eq!(console.stderr(), "foo");

            permit.flush().await.unwrap();

            assert_eq!(console.stderr(), "foobar");
            assert_eq!(console.flushed_stderr(), "foobar");
        }

        #[tokio::test]
        async fn wait_for_lock() {
            let multiplexed = create_console(&Default::default());
            let permit = multiplexed.lock().await.unwrap();
            let mut future = pin!(multiplexed.lock());

            assert!(poll!(&mut future).is_pending());

            drop(permit);

            assert!(poll!(&mut future).is_ready());
        }

        #[tokio::test]
        async fn fail_to_write() {
            let multiplexed = create_console(&FakeConsole::failing());
            let mut permit = multiplexed.lock().await.unwrap();

            assert!(permit.write_stderr(b"foo").await.is_err());
        }

        #[tokio::test]
        async fn fail_to_write_pending_outputs_on_lock() {
            let multiplexed = create_console(&FakeConsole::failing());
            let permit = multiplexed.lock().await.unwrap();

            queue_stderr(&multiplexed, "foo").await;
            drop(permit);

            assert!(multiplexed.lock().await.is_err());
        }
    }

    mod queue {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn write_stdout() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);
            let mut permit = multiplexed.queue();

            permit.write_stdout(b"foo").await.unwrap();
            permit.flush().await.unwrap();

            assert_eq!(console.stdout(), "foo");
        }

        #[tokio::test]
        async fn write_stderr() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);

            queue_stderr(&multiplexed, "foo").await;

            assert_eq!(console.stderr(), "foo");
        }

        #[tokio::test]
        async fn queue_outputs_until_flush() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);
            let mut permit = multiplexed.queue();

            permit.write_stdout(b"foo").await.unwrap();
            permit.write_stderr(b"bar").await.unwrap();

            assert_eq!(console.stdout(), "");
            assert_eq!(console.stderr(), "");

            permit.flush().await.unwrap();

            assert_eq!(console.stdout(), "foo");
            assert_eq!(console.stderr(), "bar");
        }

        #[tokio::test]
        async fn write_outputs_in_order_of_flush() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);
            let mut foo = multiplexed.queue();
            let mut bar = multiplexed.queue();

            foo.write_stderr(b"foo").await.unwrap();
            bar.write_stderr(b"bar").await.unwrap();
            bar.flush().await.unwrap();
            foo.flush().await.unwrap();

            assert_eq!(console.stderr(), "barfoo");
        }

        #[tokio::test]
        async fn queue_outputs_while_locked() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);
            let permit = multiplexed.lock().await.unwrap();

            queue_stderr(&multiplexed, "foo").await;

            assert_eq!(console.stderr(), "");

            drop(permit);
            queue_stderr(&multiplexed, "bar").await;

            assert_eq!(console.stderr(), "foobar");
        }

        #[tokio::test]
        async fn queue_outputs_on_drop() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);
            let mut permit = multiplexed.queue();

            permit.write_stderr(b"foo").await.unwrap();
            drop(permit);

            assert_eq!(console.stderr(), "");

            queue_stderr(&multiplexed, "bar").await;

            assert_eq!(console.stderr(), "foobar");
        }

        #[tokio::test]
        async fn flush_nothing() {
            let console = FakeConsole::default();
            let multiplexed = create_console(&console);

            multiplexed.queue().flush().await.unwrap();

            assert_eq!(console.stdout(), "");
            assert_eq!(console.stderr(), "");
        }

        #[tokio::test]
        async fn fail_to_flush() {
            let multiplexed = create_console(&FakeConsole::failing());
            let mut permit = multiplexed.queue();

            permit.write_stderr(b"foo").await.unwrap();

            assert!(permit.flush().await.is_err());
        }
    }
}
