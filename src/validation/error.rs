use itertools::Itertools;
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    sync::Arc,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationError {
    CircularBuildDependency(Vec<Arc<str>>),
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
                        .dedup()
                        .map(|string| string.as_ref())
                        .collect::<Vec<&str>>()
                        .join(" -> ")
                )
            }
            Self::CircularModuleDependency => {
                write!(formatter, "build file dependency cycle detected")
            }
        }
    }
}
