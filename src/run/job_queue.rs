use alloc::collections::BTreeMap;
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use std::sync::{Mutex, PoisonError};
use tokio::sync::oneshot::{self, Receiver, Sender, error::RecvError};

// A job queue.
#[derive(Debug)]
pub struct JobQueue {
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    free: usize,
    waiters: BTreeMap<usize, Sender<()>>,
}

impl JobQueue {
    pub const fn new(limit: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                free: limit,
                waiters: BTreeMap::new(),
            }),
        }
    }

    pub async fn acquire(&self, priority: usize) -> Result<JobPermit<'_>, RecvError> {
        if let Some(receiver) = self.wait(priority) {
            Waiter {
                queue: self,
                receiver,
            }
            .await?;
        }

        Ok(JobPermit { queue: self })
    }

    fn wait(&self, priority: usize) -> Option<Receiver<()>> {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);

        if state.free > 0 {
            state.free -= 1;

            None
        } else {
            let (sender, receiver) = oneshot::channel();

            state.waiters.insert(priority, sender);

            Some(receiver)
        }
    }

    fn release(&self) {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);

        while let Some((_, sender)) = state.waiters.pop_first() {
            if sender.send(()).is_ok() {
                return;
            }
        }

        state.free += 1;
    }
}

struct Waiter<'a> {
    queue: &'a JobQueue,
    receiver: Receiver<()>,
}

impl Future for Waiter<'_> {
    type Output = Result<(), RecvError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.receiver).poll(context)
    }
}

impl Drop for Waiter<'_> {
    fn drop(&mut self) {
        if self.receiver.try_recv().is_ok() {
            self.queue.release();
        }
    }
}

pub struct JobPermit<'a> {
    queue: &'a JobQueue,
}

impl Drop for JobPermit<'_> {
    fn drop(&mut self) {
        self.queue.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::pin::pin;
    use futures::poll;

    #[tokio::test]
    async fn acquire_slots_within_limit() {
        let queue = JobQueue::new(2);
        let _foo = queue.acquire(0).await.unwrap();
        let _bar = queue.acquire(1).await.unwrap();
    }

    #[tokio::test]
    async fn wait_for_slot() {
        let queue = JobQueue::new(1);
        let permit = queue.acquire(0).await.unwrap();
        let mut future = pin!(queue.acquire(1));

        assert!(poll!(&mut future).is_pending());

        drop(permit);

        assert!(poll!(&mut future).is_ready());
    }

    #[tokio::test]
    async fn admit_waiters_by_priority() {
        let queue = JobQueue::new(1);
        let permit = queue.acquire(0).await.unwrap();
        let mut second = pin!(queue.acquire(2));
        let mut first = pin!(queue.acquire(1));

        assert!(poll!(&mut second).is_pending());
        assert!(poll!(&mut first).is_pending());

        drop(permit);

        assert!(poll!(&mut second).is_pending());

        let Poll::Ready(Ok(permit)) = poll!(&mut first) else {
            panic!("slot not handed over");
        };

        assert!(poll!(&mut second).is_pending());

        drop(permit);

        assert!(poll!(&mut second).is_ready());
    }

    #[tokio::test]
    async fn skip_dropped_waiter() {
        let queue = JobQueue::new(1);
        let permit = queue.acquire(0).await.unwrap();
        let mut first = Box::pin(queue.acquire(1));
        let mut second = pin!(queue.acquire(2));

        assert!(poll!(&mut first).is_pending());
        assert!(poll!(&mut second).is_pending());

        drop(first);
        drop(permit);

        assert!(poll!(&mut second).is_ready());
    }

    #[tokio::test]
    async fn release_slot_handed_over_to_dropped_waiter() {
        let queue = JobQueue::new(1);
        let permit = queue.acquire(0).await.unwrap();
        let mut first = Box::pin(queue.acquire(1));

        assert!(poll!(&mut first).is_pending());

        drop(permit);
        drop(first);

        assert!(poll!(pin!(queue.acquire(2))).is_ready());
    }
}
