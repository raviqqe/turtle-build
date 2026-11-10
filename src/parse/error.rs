use thiserror::Error;
use winnow::error::ContextError;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{message}")]
pub struct ParseError {
    message: String,
}

impl ParseError {
    pub const fn new(message: String) -> Self {
        Self { message }
    }
}

impl From<winnow::error::ParseError<&str, ContextError>> for ParseError {
    fn from(error: winnow::error::ParseError<&str, ContextError>) -> Self {
        Self::new(error.to_string())
    }
}
