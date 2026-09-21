mod file_path;

pub use self::file_path::FilePath;
use alloc::sync::Arc;
#[cfg(test)]
use once_cell::sync::Lazy;
use scc::HashSet;

// Tests share a pool so that paths of the same string are identical in them.
#[cfg(test)]
pub static TEST_PATH_POOL: Lazy<Arc<PathPool>> = Lazy::new(Default::default);

/// A path pool.
#[derive(Debug, Default)]
pub struct PathPool {
    paths: HashSet<Arc<str>>,
}

impl PathPool {
    /// Creates a path pool.
    pub fn new() -> Self {
        Self::default()
    }

    /// Interns a path.
    pub fn intern(&self, path: &str) -> FilePath {
        self.paths
            .read_sync(path, |path| FilePath::new(path.clone()))
            .unwrap_or_else(|| {
                // Another thread might intern the same path at the same time.
                let _ = self.paths.insert_sync(path.into());

                self.intern(path)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;
    use pretty_assertions::assert_eq;

    #[test]
    fn intern_path() {
        assert_eq!(PathPool::new().intern("foo").as_str(), "foo");
    }

    #[test]
    fn intern_same_path_twice() {
        let pool = PathPool::new();

        assert!(ptr::eq(
            pool.intern("foo").as_str(),
            pool.intern("foo").as_str()
        ));
    }

    #[test]
    fn intern_different_paths() {
        let pool = PathPool::new();

        assert_eq!(pool.intern("foo").as_str(), "foo");
        assert_eq!(pool.intern("bar").as_str(), "bar");
    }

    #[test]
    fn intern_same_path_after_different_path() {
        let pool = PathPool::new();
        let path = pool.intern("foo");

        pool.intern("bar");

        assert!(ptr::eq(path.as_str(), pool.intern("foo").as_str()));
    }

    #[test]
    fn intern_path_in_test_pool() {
        assert!(ptr::eq(
            FilePath::from("foo").as_str(),
            TEST_PATH_POOL.intern("foo").as_str()
        ));
    }
}
