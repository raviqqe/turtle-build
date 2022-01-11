use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationError {
    CircularOutputDependency,
    CircularModuleDependency,
}

impl Error for ValidationError {}

impl Display for ValidationError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::CircularOutputDependency => {
                write!(formatter, "circular output dependency detected")
            }
            Self::CircularModuleDependency => {
                write!(formatter, "circular module dependency detected")
            }
        }
    }
}
