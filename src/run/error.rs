use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug)]
pub enum InfrastructureError {
    ChildExit(Option<i32>),
}

impl Error for InfrastructureError {}

impl Display for InfrastructureError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::ChildExit(Some(code)) => {
                write!(formatter, "child process exited with status code {}", code)
            }
            Self::ChildExit(None) => {
                write!(formatter, "child process exited without status code")
            }
        }
    }
}
