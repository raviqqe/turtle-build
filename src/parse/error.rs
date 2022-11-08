use std::{
    error::Error,
    fmt::{self, Display},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    message: String,
}

impl ParseError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<nom::Err<nom::error::Error<&str>>> for ParseError {
    fn from(error: nom::Err<nom::error::Error<&str>>) -> Self {
        Self::new(format!("{}", error))
    }
}

impl Error for ParseError {}

impl Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "{}", &self.message)
    }
}
