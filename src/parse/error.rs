use combine::easy::Errors;
use std::{
    error::Error,
    fmt::{self, Display},
};

#[derive(Debug, PartialEq)]
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

impl<P: Display> From<Errors<char, &str, P>> for ParseError {
    fn from(errors: Errors<char, &str, P>) -> Self {
        Self::new(format!("{}", errors))
    }
}

impl Error for ParseError {}

impl Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "{}", &self.message)
    }
}
