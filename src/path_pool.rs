use alloc::sync::Arc;
use scc::HashSet;

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
    pub fn intern(&self, path: &str) -> Arc<str> {
        self.paths.read_sync(path, Clone::clone).unwrap_or_else(|| {
            // Another thread might intern the same path at the same time.
            let _ = self.paths.insert_sync(path.into());

            self.intern(path)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn intern_path() {
        assert_eq!(PathPool::new().intern("foo"), "foo".into());
    }

    #[test]
    fn intern_same_path_twice() {
        let pool = PathPool::new();

        assert!(Arc::ptr_eq(&pool.intern("foo"), &pool.intern("foo")));
    }

    #[test]
    fn intern_different_paths() {
        let pool = PathPool::new();

        assert_eq!(pool.intern("foo"), "foo".into());
        assert_eq!(pool.intern("bar"), "bar".into());
    }

    #[test]
    fn intern_same_path_after_different_path() {
        let pool = PathPool::new();
        let path = pool.intern("foo");

        pool.intern("bar");

        assert!(Arc::ptr_eq(&path, &pool.intern("foo")));
    }
}
