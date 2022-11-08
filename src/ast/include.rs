#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Include<'a> {
    path: &'a str,
}

impl<'a> Include<'a> {
    pub fn new(path: &'a str) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &str {
        self.path
    }
}
