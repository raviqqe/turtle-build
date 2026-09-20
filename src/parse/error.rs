use thiserror::Error;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{message}")]
pub struct ParseError {
    message: String,
}

impl ParseError {
    pub fn new(message: String) -> Self {
        Self {
            message,
        }
    }
}

impl From<nom::Err<nom::error::Error<&str>>> for ParseError {
    fn from(error: nom::Err<nom::error::Error<&str>>) -> Self {
        Self::new(error.to_string())
    }
}
