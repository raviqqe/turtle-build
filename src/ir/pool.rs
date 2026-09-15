use alloc::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pool {
    Console,
    Limited { name: Arc<str> },
}
