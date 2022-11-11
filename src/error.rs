use crate::{compile::CompileError, ir::Build, parse::ParseError, validation::ValidationError};
use fnv::FnvHashMap;
use itertools::Itertools;
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    sync::Arc,
};
use tokio::{io, sync::AcquireError, task::JoinError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationError {
    Build,
    Compile(CompileError),
    DefaultOutputNotFound(Arc<str>),
    DynamicDependencyNotFound(Arc<Build>),
    InputNotBuilt(String),
    InputNotFound(String),
    Other(String),
    Parse(ParseError),
    Sled(sled::Error),
    Validation(ValidationError),
}

impl ApplicationError {
    pub fn map_outputs(self, source_map: &FnvHashMap<Arc<str>, Arc<str>>) -> Self {
        match self {
            Self::Validation(ValidationError::CircularBuildDependency(outputs)) => {
                Self::Validation(ValidationError::CircularBuildDependency(
                    outputs
                        .into_iter()
                        .map(|output| source_map.get(&output).cloned().unwrap_or(output))
                        .dedup()
                        .collect(),
                ))
            }
            _ => self,
        }
    }
}

impl Error for ApplicationError {}

impl Display for ApplicationError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::Build => write!(formatter, "build failed"),
            Self::Compile(error) => write!(formatter, "{}", error),
            Self::DefaultOutputNotFound(output) => {
                write!(formatter, "default output \"{}\" not found", output)
            }
            Self::DynamicDependencyNotFound(build) => {
                write!(
                    formatter,
                    "outputs {} not found in dynamic dependency file {}",
                    build.outputs().join(", "),
                    build.dynamic_module().unwrap()
                )
            }
            Self::InputNotBuilt(input) => {
                write!(formatter, "input \"{}\" not built yet", input)
            }
            Self::InputNotFound(input) => {
                write!(formatter, "input \"{}\" not found", input)
            }
            Self::Other(message) => write!(formatter, "{}", message),
            Self::Parse(error) => write!(formatter, "{}", error),
            Self::Sled(error) => write!(formatter, "{}", error),
            Self::Validation(error) => write!(formatter, "{}", error),
        }
    }
}

impl From<AcquireError> for ApplicationError {
    fn from(error: AcquireError) -> Self {
        Self::Other(format!("{}", &error))
    }
}

impl From<bincode::Error> for ApplicationError {
    fn from(error: bincode::Error) -> Self {
        Self::Other(format!("{}", &error))
    }
}

impl From<Box<dyn Error>> for ApplicationError {
    fn from(error: Box<dyn Error>) -> Self {
        Self::Other(error.to_string())
    }
}

impl From<CompileError> for ApplicationError {
    fn from(error: CompileError) -> Self {
        Self::Compile(error)
    }
}

impl From<io::Error> for ApplicationError {
    fn from(error: io::Error) -> Self {
        Self::Other(format!("{}", &error))
    }
}

impl From<JoinError> for ApplicationError {
    fn from(error: JoinError) -> Self {
        Self::Other(format!("{}", &error))
    }
}

impl From<ParseError> for ApplicationError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl From<sled::Error> for ApplicationError {
    fn from(error: sled::Error) -> Self {
        Self::Sled(error)
    }
}

impl From<ValidationError> for ApplicationError {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_dependency_cycle_error() {
        assert_eq!(
            ApplicationError::from(ValidationError::CircularBuildDependency(vec![
                "foo.o".into(),
                "bar.o".into()
            ]))
            .map_outputs(
                &[
                    ("foo.o".into(), "foo.c".into()),
                    ("bar.o".into(), "bar.c".into())
                ]
                .into_iter()
                .collect()
            ),
            ApplicationError::from(ValidationError::CircularBuildDependency(vec![
                "foo.c".into(),
                "bar.c".into()
            ]))
        );
    }

    #[test]
    fn map_dependency_cycle_error_with_duplicate_sources() {
        assert_eq!(
            ApplicationError::from(ValidationError::CircularBuildDependency(vec![
                "foo.o".into(),
                "foo.h".into()
            ]))
            .map_outputs(
                &[
                    ("foo.o".into(), "foo.c".into()),
                    ("foo.h".into(), "foo.c".into())
                ]
                .into_iter()
                .collect()
            ),
            ApplicationError::from(ValidationError::CircularBuildDependency(vec![
                "foo.c".into()
            ]))
        );
    }
}
