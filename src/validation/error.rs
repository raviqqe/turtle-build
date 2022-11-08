use itertools::Itertools;
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationError<'a> {
    CircularBuildDependency(Vec<&'a str>),
    CircularModuleDependency,
}

impl<'a> Error for ValidationError<'a> {}

impl<'a> Display for ValidationError<'a> {
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
                        .copied()
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
