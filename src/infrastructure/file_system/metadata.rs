use std::fs;

pub struct Metadata {
    directory: bool,
}

impl Metadata {
    pub const fn new(directory: bool) -> Self {
        Self { directory }
    }

    pub const fn is_file(&self) -> bool {
        !self.directory
    }
}

impl From<fs::Metadata> for Metadata {
    fn from(metadata: fs::Metadata) -> Self {
        Self::new(metadata.is_dir())
    }
}
