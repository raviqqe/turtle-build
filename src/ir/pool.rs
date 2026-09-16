use alloc::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pool {
    Console,
    Limited(Arc<str>),
}
