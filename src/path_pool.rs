use crate::path_id::PathId;
use fnv::FnvHashMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathPool {
    paths: Vec<&'static str>,
    path_ids: FnvHashMap<&'static str, PathId>,
}

impl PathPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: usize) -> &str {
        self.paths[id]
    }

    pub fn insert(&mut self, path: &'static str) -> PathId {
        if let Some(&id) = self.path_ids.get(path) {
            return id;
        }

        self.paths.push(path);

        let id = PathId::new(self.paths.len() - 1);

        self.path_ids.insert(path, id);

        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_path() {
        let mut pool = PathPool::new();

        pool.insert("foo");

        assert_eq!(pool.insert("bar"), PathId::new(1));
    }

    #[test]
    fn insert_paths() {
        let mut pool = PathPool::new();

        assert_eq!(pool.insert("foo"), PathId::new(0));
        assert_eq!(pool.insert("bar"), PathId::new(1));
    }

    #[test]
    fn insert_same_paths() {
        let mut pool = PathPool::new();

        assert_eq!(pool.insert("foo"), PathId::new(0));
        assert_eq!(pool.insert("foo"), PathId::new(0));
    }
}
