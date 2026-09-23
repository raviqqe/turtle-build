use alloc::collections::BTreeMap;
use std::sync::Mutex;
use tokio::sync::oneshot::{self, Receiver, Sender};

// A job queue.
#[derive(Debug)]
pub struct JobQueue {
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    free_slots: usize,
    waiters: BTreeMap<(usize, usize), Sender<()>>,
    arrival: usize,
}

impl JobQueue {
    pub const fn new(limit: usize) -> Self {
        Self {
            state: Mutex::new(State {
                free_slots: limit,
                waiters: BTreeMap::new(),
                arrival: 0,
            }),
        }
    }

    pub async fn acquire(&self, sequence: usize) -> JobPermit<'_> {
        if let Some(receiver) = self.wait(sequence) {
            let mut waiter = Waiter {
                queue: self,
                receiver: Some(receiver),
            };

            waiter
                .receiver
                .as_mut()
                .unwrap()
                .await
                .expect("job queue alive");
            waiter.receiver = None;
        }

        JobPermit { queue: self }
    }

    fn wait(&self, sequence: usize) -> Option<Receiver<()>> {
        let mut state = self.state.lock().unwrap();
        let State {
            free_slots,
            waiters,
            arrival,
        } = &mut *state;

        if *free_slots > 0 {
            *free_slots -= 1;

            None
        } else {
            let (sender, receiver) = oneshot::channel();

            *arrival += 1;
            waiters.insert((sequence, *arrival), sender);

            Some(receiver)
        }
    }

    fn release(&self) {
        let mut state = self.state.lock().unwrap();

        while let Some((_, sender)) = state.waiters.pop_first() {
            if sender.send(()).is_ok() {
                return;
            }
        }

        state.free_slots += 1;
    }
}

struct Waiter<'a> {
    queue: &'a JobQueue,
    receiver: Option<Receiver<()>>,
}

impl Drop for Waiter<'_> {
    fn drop(&mut self) {
        if let Some(mut receiver) = self.receiver.take()
            && receiver.try_recv().is_ok()
        {
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
    use core::{pin::pin, task::Poll};
    use futures::poll;

    #[tokio::test]
    async fn acquire_slots_within_limit() {
        let queue = JobQueue::new(2);
        let _foo = queue.acquire(0).await;
        let _bar = queue.acquire(1).await;
    }

    #[tokio::test]
    async fn wait_for_slot() {
        let queue = JobQueue::new(1);
        let permit = queue.acquire(0).await;
        let mut future = pin!(queue.acquire(1));

        assert!(poll!(&mut future).is_pending());

        drop(permit);

        assert!(poll!(&mut future).is_ready());
    }

    #[tokio::test]
    async fn admit_waiters_in_sequence_order() {
        let queue = JobQueue::new(1);
        let permit = queue.acquire(0).await;
        let mut second = pin!(queue.acquire(2));
        let mut first = pin!(queue.acquire(1));

        assert!(poll!(&mut second).is_pending());
        assert!(poll!(&mut first).is_pending());

        drop(permit);

        assert!(poll!(&mut second).is_pending());

        let Poll::Ready(permit) = poll!(&mut first) else {
            panic!("slot not handed over");
        };

        assert!(poll!(&mut second).is_pending());

        drop(permit);

        assert!(poll!(&mut second).is_ready());
    }

    #[tokio::test]
    async fn admit_waiters_of_same_sequence_in_arrival_order() {
        let queue = JobQueue::new(1);
        let permit = queue.acquire(0).await;
        let mut first = pin!(queue.acquire(1));
        let mut second = pin!(queue.acquire(1));

        assert!(poll!(&mut first).is_pending());
        assert!(poll!(&mut second).is_pending());

        drop(permit);

        assert!(poll!(&mut second).is_pending());

        let Poll::Ready(permit) = poll!(&mut first) else {
            panic!("slot not handed over");
        };

        assert!(poll!(&mut second).is_pending());

        drop(permit);

        assert!(poll!(&mut second).is_ready());
    }

    #[tokio::test]
    async fn skip_dropped_waiter() {
        let queue = JobQueue::new(1);
        let permit = queue.acquire(0).await;
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
        let permit = queue.acquire(0).await;
        let mut first = Box::pin(queue.acquire(1));

        assert!(poll!(&mut first).is_pending());

        drop(permit);
        drop(first);

        assert!(poll!(pin!(queue.acquire(2))).is_ready());
    }
}
