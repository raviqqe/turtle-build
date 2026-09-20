#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Include {
    path: String,
}

impl Include {
    pub const fn new(path: String) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}
