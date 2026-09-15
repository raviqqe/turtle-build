use alloc::sync::Arc;
use core::num::NonZeroUsize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pool {
    Console,
    Limited { name: Arc<str>, depth: NonZeroUsize },
}
