use core::{
    error::Error,
    fmt::{self, Display, Formatter},
};
use std::io;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsoleError(String);

impl ConsoleError {
    pub fn new(error: impl Display) -> Self {
        Self(error.to_string())
    }
}

impl Error for ConsoleError {}

impl Display for ConsoleError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{}", self.0)
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
