use std::{fs, io, time::SystemTime};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metadata {
    modified_time: SystemTime,
    directory: bool,
}

impl Metadata {
    pub const fn new(modified_time: SystemTime, directory: bool) -> Self {
        Self {
            modified_time,
            directory,
        }
    }

    pub const fn modified_time(&self) -> SystemTime {
        self.modified_time
    }

    pub const fn is_file(&self) -> bool {
        !self.directory
    }
}

impl TryFrom<fs::Metadata> for Metadata {
    type Error = io::Error;

    fn try_from(metadata: fs::Metadata) -> Result<Self, Self::Error> {
        Ok(Self::new(metadata.modified()?, metadata.is_dir()))
    }
}
