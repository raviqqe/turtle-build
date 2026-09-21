use alloc::sync::Arc;
use core::fmt::{self, Display, Formatter};
use std::path::Path;

/// A file path.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FilePath(Arc<str>);

impl FilePath {
    pub(super) const fn new(path: Arc<str>) -> Self {
        Self(path)
    }

    /// Returns a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<Path> for FilePath {
    fn as_ref(&self) -> &Path {
        self.as_str().as_ref()
    }
}

impl Display for FilePath {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        self.as_str().fmt(formatter)
    }
}

#[cfg(test)]
impl From<&str> for FilePath {
    fn from(path: &str) -> Self {
        super::TEST_PATH_POOL.intern(path)
    }
}
