#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathId(usize);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathSet<'a> {
    paths: Vec<&'a str>,
}

impl<'a> PathSet<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn path(&self, id: usize) -> &'a str {
        self.paths[id]
    }

    pub fn insert(&mut self, path: &'a str) -> PathId {
        self.paths.push(path);

        PathId(self.paths.len() - 1)
    }
}
