#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pool {
    name: String,
    depth: String,
}

impl Pool {
    pub fn new(name: impl Into<String>, depth: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            depth: depth.into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn depth(&self) -> &str {
        &self.depth
    }
}
