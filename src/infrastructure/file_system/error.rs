use core::fmt::Display;
use std::io;
use thiserror::Error;
use tokio::sync::AcquireError;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{0}")]
pub struct FileError(String);

impl FileError {
    pub fn new(error: impl Display) -> Self {
        Self(error.to_string())
    }
}

impl From<AcquireError> for FileError {
    fn from(error: AcquireError) -> Self {
        Self::new(error)
    }
}

impl From<io::Error> for FileError {
    fn from(error: io::Error) -> Self {
        Self::new(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display() {
        assert_eq!(
            FileError::new("file not found").to_string(),
            "file not found"
        );
    }
}
