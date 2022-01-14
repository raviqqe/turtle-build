use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationError {
    CircularBuildDependency(Vec<String>),
    CircularModuleDependency,
}

impl Error for ValidationError {}

impl Display for ValidationError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::CircularBuildDependency(_) => {
                write!(formatter, "circular build dependency detected")
            }
            Self::CircularModuleDependency => {
                write!(formatter, "circular build file dependency detected")
            }
        }
    }
}
