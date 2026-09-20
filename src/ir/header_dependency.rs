use alloc::sync::Arc;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum HeaderDependency {
    Make { path: Arc<str> },
    Gcc { path: Arc<str> },
    Msvc { prefix: String },
}
