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
            Self::CircularBuildDependency(cycle) => {
                write!(
                    formatter,
                    "dependency cycle detected: {}",
                    cycle
                        .iter()
                        .chain(cycle.first())
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(" -> ")
                )
            }
            Self::CircularModuleDependency => {
                write!(formatter, "build file dependency cycle detected")
            }
        }
    }
}
