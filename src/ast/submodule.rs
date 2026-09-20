#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submodule {
    path: String,
}

impl Submodule {
    pub fn new(path: String) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}
