use fnv::FnvHashMap;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct PathId(usize);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathSet<'a> {
    paths: Vec<&'a str>,
    path_ids: FnvHashMap<&'a str, PathId>,
}

impl<'a> PathSet<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn path(&self, id: usize) -> &'a str {
        self.paths[id]
    }

    pub fn insert(&mut self, path: &'a str) -> PathId {
        if let Some(&id) = self.path_ids.get(path) {
            return id;
        }

        self.paths.push(path);

        let id = PathId(self.paths.len() - 1);

        self.path_ids.insert(path, id);

        id
    }
}
