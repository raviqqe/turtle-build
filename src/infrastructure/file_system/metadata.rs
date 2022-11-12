use std::{fs, io, time::SystemTime};

pub struct Metadata {
    modified_time: SystemTime,
    directory: bool,
}

impl Metadata {
    pub fn new(modified_time: SystemTime, directory: bool) -> Self {
        Self {
            modified_time,
            directory,
        }
    }

    pub fn modified_time(&self) -> SystemTime {
        self.modified_time
    }

    pub fn is_file(&self) -> bool {
        !self.directory
    }
}

impl TryFrom<fs::Metadata> for Metadata {
    type Error = io::Error;

    fn try_from(metadata: fs::Metadata) -> Result<Self, Self::Error> {
        Ok(Metadata::new(metadata.modified()?, metadata.is_dir()))
    }
}
