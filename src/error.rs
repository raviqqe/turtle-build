use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    path::Path,
};
use tokio::{io, task::JoinError};

#[derive(Clone, Debug)]
pub enum InfrastructureError {
    CommandExit(String, Option<i32>),
    DefaultOutputNotFound(String),
    Other(String),
    Sled(sled::Error),
}

impl InfrastructureError {
    pub fn with_path(error: io::Error, path: impl AsRef<Path>) -> Self {
        Self::Other(format!("{}: {}", error, path.as_ref().display()))
    }
}

impl Error for InfrastructureError {}

impl Display for InfrastructureError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::CommandExit(command, code) => {
                write!(
                    formatter,
                    "command exited {}: {}",
                    if let Some(code) = code {
                        format!("with status code {}", code)
                    } else {
                        "without status code".into()
                    },
                    command
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

impl From<io::Error> for InfrastructureError {
    fn from(error: io::Error) -> Self {
        Self::Other(format!("{}", &error))
    }
}

impl From<JoinError> for InfrastructureError {
    fn from(error: JoinError) -> Self {
        Self::Other(format!("{}", &error))
    }
}

impl From<sled::Error> for InfrastructureError {
    fn from(error: sled::Error) -> Self {
        Self::Sled(error)
    }
}
