use crate::{
    build_graph::BuildGraphError, compile::CompileError, ir::Build,
    module_dependency::ModuleDependencyError, parse::ParseError,
};
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    sync::Arc,
};
use tokio::{io, task::JoinError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationError {
    Build,
    BuildGraph(BuildGraphError),
    Compile(CompileError),
    DefaultOutputNotFound(Arc<str>),
    DynamicDependencyNotFound(Arc<Build>),
    FileNotFound(String),
    InputNotBuilt(String),
    InputNotFound(String),
    ModuleDependency(ModuleDependencyError),
    Other(String),
    OutputNotFound(String),
    Parse(ParseError),
    Sled(sled::Error),
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
            Self::ModuleDependency(error) => {
                write!(formatter, "{}", error)
            }
            Self::Other(message) => write!(formatter, "{}", message),
            Self::OutputNotFound(output) => {
                write!(formatter, "output \"{}\" not found", output)
            }
            Self::Parse(error) => write!(formatter, "{}", error),
            Self::Sled(error) => write!(formatter, "{}", error),
            Self::BuildGraph(error) => write!(formatter, "{}", error),
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

impl From<ModuleDependencyError> for ApplicationError {
    fn from(error: ModuleDependencyError) -> Self {
        Self::ModuleDependency(error)
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

impl From<BuildGraphError> for ApplicationError {
    fn from(error: BuildGraphError) -> Self {
        Self::BuildGraph(error)
    }
}
