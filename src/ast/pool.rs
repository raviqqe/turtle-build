#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pool {
    name: String,
    depth: String,
}

impl Pool {
    pub fn new(name: String, depth: String) -> Self {
        Self {
            name,
            depth,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn depth(&self) -> &str {
        &self.depth
    }
}
