use bincode::error::{DecodeError, EncodeError};
use core::{
    error::Error,
    fmt::{self, Display, Formatter},
    str::Utf8Error,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseError(String);

impl DatabaseError {
    pub fn new(error: impl Display) -> Self {
        Self(error.to_string())
    }
}

impl Error for DatabaseError {}

impl Display for DatabaseError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{}", self.0)
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
