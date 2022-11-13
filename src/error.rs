use crate::{compile::CompileError, ir::Build, parse::ParseError, validation::ValidationError};
use itertools::Itertools;
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    sync::Arc,
};
use tokio::{io, task::JoinError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationError {
    Build,
    Compile(CompileError),
    DefaultOutputNotFound(Arc<str>),
    DynamicDependencyNotFound(Arc<Build>),
    FileNotFound(String),
    InputNotBuilt(String),
    InputNotFound(String),
    Other(String),
    Parse(ParseError),
    Sled(sled::Error),
    Validation(ValidationError),
}

impl ApplicationError {
    pub fn map_outputs<E: Into<Self>>(
        self,
        map_path: impl Fn(&str) -> Result<Option<String>, E>,
    ) -> Self {
        match &self {
            Self::FileNotFound(path) => match map_path(&path) {
                Ok(path) => {
                    if let Some(path) = path {
                        Self::FileNotFound(path)
                    } else {
                        self
                    }
                }
                Err(error) => error.into(),
            },
            Self::Validation(ValidationError::CircularBuildDependency(outputs)) => {
                match outputs
                    .iter()
                    .map(|output| -> Result<_, E> {
                        Ok(map_path(output)?
                            .map(|string| string.into())
                            .unwrap_or(output.clone()))
                    })
                    .collect::<Result<Vec<_>, _>>()
                {
                    Ok(outputs) => Self::Validation(ValidationError::CircularBuildDependency(
                        outputs.into_iter().dedup().collect(),
                    )),
                    Err(error) => error.into(),
                }
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
            Self::FileNotFound(path) => write!(formatter, "file \"{}\" not found", path),
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
        Self::Other(error.to_string())
    }
}

impl From<JoinError> for ApplicationError {
    fn from(error: JoinError) -> Self {
        Self::Other(error.to_string())
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
            .map_outputs(|output| Ok::<_, ApplicationError>(match output {
                "foo.o" => Some("foo.c".into()),
                "bar.o" => Some("bar.c".into()),
                _ => None,
            })),
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
            .map_outputs(|output| Ok::<_, ApplicationError>(match output {
                "foo.o" => Some("foo.c".into()),
                "foo.h" => Some("foo.c".into()),
                _ => None,
            })),
            ApplicationError::from(ValidationError::CircularBuildDependency(vec![
                "foo.c".into()
            ]))
        );
    }
}
