#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submodule<'a> {
    path: &'a str,
}

impl<'a> Submodule<'a> {
    pub fn new(path: &'a str) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &'a str {
        self.path
    }
}
