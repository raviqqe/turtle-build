use core::fmt::Display;
use std::io;
use thiserror::Error;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{0}")]
pub struct CommandError(String);

impl CommandError {
    pub fn new(error: impl Display) -> Self {
        Self(error.to_string())
    }
}

impl From<io::Error> for CommandError {
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
            CommandError::new("command not found").to_string(),
            "command not found"
        );
    }
}
