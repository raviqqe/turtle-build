use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};
use tokio::{io, task::JoinError};

#[derive(Clone, Debug)]
pub enum RunError {
    ChildExit(Option<i32>),
    Other(String),
}

impl Error for RunError {}

impl Display for RunError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::ChildExit(Some(code)) => {
                write!(formatter, "child process exited with status code {}", code)
            }
            Self::ChildExit(None) => {
                write!(formatter, "child process exited without status code")
            }
            Self::Other(message) => {
                write!(formatter, "{}", message)
            }
        }
    }
}

impl From<io::Error> for RunError {
    fn from(error: io::Error) -> Self {
        Self::Other(format!("{}", &error))
    }
}

impl From<JoinError> for RunError {
    fn from(error: JoinError) -> Self {
        Self::Other(format!("{}", &error))
    }
}