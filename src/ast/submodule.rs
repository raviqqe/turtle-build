#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submodule {
    path: String,
}

impl Submodule {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}
