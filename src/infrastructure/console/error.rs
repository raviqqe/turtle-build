use core::fmt::Display;
use std::io;
use thiserror::Error;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{0}")]
pub struct ConsoleError(String);

impl ConsoleError {
    pub fn new(error: impl Display) -> Self {
        Self(error.to_string())
    }
}

impl From<io::Error> for ConsoleError {
    fn from(error: io::Error) -> Self {
        Self::new(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display() {
        assert_eq!(ConsoleError::new("broken pipe").to_string(), "broken pipe");
    }
}
