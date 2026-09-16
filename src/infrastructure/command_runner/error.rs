use core::{
    error::Error,
    fmt::{self, Display, Formatter},
};
use std::io;
use tokio::sync::AcquireError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandError(String);

impl CommandError {
    pub fn new(error: impl Display) -> Self {
        Self(error.to_string())
    }
}

impl Error for CommandError {}

impl Display for CommandError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl From<AcquireError> for CommandError {
    fn from(error: AcquireError) -> Self {
        Self::new(error)
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
