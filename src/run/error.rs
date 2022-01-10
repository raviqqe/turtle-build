use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    path::Path,
};
use tokio::{io, task::JoinError};

#[derive(Clone, Debug)]
pub enum RunError {
    ChildExit(Option<i32>),
    DefaultOutputNotFound(String),
    Other(String),
    Sled(sled::Error),
}

impl RunError {
    pub fn with_path(error: io::Error, path: impl AsRef<Path>) -> Self {
        Self::Other(format!("{}: {}", path.as_ref().display(), error))
    }

    pub fn with_command(error: io::Error, command: &str) -> Self {
        Self::Other(format!("{}: {}", command, error))
    }
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
