use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};
use tokio::{io, task::JoinError};

#[derive(Clone, Debug)]
pub enum RunError {
    ChildExit(Option<i32>),
    DefaultOutputNotFound(String),
    Other(String),
    Sled(sled::Error),
}

impl Error for RunError {}

impl Display for RunError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::ChildExit(code) => {
                write!(
                    formatter,
                    "child process exited {}",
                    if let Some(code) = code {
                        format!("with status code {}", code)
                    } else {
                        "without status code".into()
                    }
                )
            }
            Self::DefaultOutputNotFound(output) => {
                write!(formatter, "default output \"{}\" not found", output)
            }
            Self::Other(message) => write!(formatter, "{}", message),
            Self::Sled(error) => write!(formatter, "{}", error),
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

impl From<sled::Error> for RunError {
    fn from(error: sled::Error) -> Self {
        Self::Sled(error)
    }
}
