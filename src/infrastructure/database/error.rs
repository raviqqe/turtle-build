use bincode::error::{DecodeError, EncodeError};
use core::{fmt::Display, str::Utf8Error};
use thiserror::Error;

/// A database error.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{0}")]
pub struct DatabaseError(String);

impl DatabaseError {
    /// Creates an error.
    pub fn new(error: impl Display) -> Self {
        Self(error.to_string())
    }
}

impl From<DecodeError> for DatabaseError {
    fn from(error: DecodeError) -> Self {
        Self::new(error)
    }
}

impl From<EncodeError> for DatabaseError {
    fn from(error: EncodeError) -> Self {
        Self::new(error)
    }
}

impl From<fjall::Error> for DatabaseError {
    fn from(error: fjall::Error) -> Self {
        Self::new(error)
    }
}

impl From<Utf8Error> for DatabaseError {
    fn from(error: Utf8Error) -> Self {
        Self::new(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display() {
        assert_eq!(
            DatabaseError::new("database not initialized").to_string(),
            "database not initialized"
        );
    }
}
