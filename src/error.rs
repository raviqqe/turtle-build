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
pub enum BuildError {
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
}

impl Error for BuildError {}

impl Display for BuildError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::Build => write!(formatter, "build failed"),
            Self::Compile(error) => write!(formatter, "{error}"),
            Self::DefaultOutputNotFound(output) => {
                write!(formatter, "default output \"{output}\" not found")
            }
            Self::DynamicDependencyNotFound(build) => {
                write!(
                    formatter,
                    "outputs {} not found in dynamic dependency file {}",
                    build.outputs().join(", "),
                    build.dynamic_module().unwrap()
                )
            }
            Self::FileNotFound(path) => write!(formatter, "file \"{path}\" not found"),
            Self::InputNotBuilt(input) => {
                write!(formatter, "input \"{input}\" not built yet")
            }
            Self::InputNotFound(input) => {
                write!(formatter, "input \"{input}\" not found")
            }
            Self::ModuleDependency(error) => {
                write!(formatter, "{error}")
            }
            Self::Other(message) => write!(formatter, "{message}"),
            Self::OutputNotFound(output) => {
                write!(formatter, "output \"{output}\" not found")
            }
            Self::Parse(error) => write!(formatter, "{error}"),
            Self::BuildGraph(error) => write!(formatter, "{error}"),
        }
    }
}

impl From<Box<dyn Error>> for BuildError {
    fn from(error: Box<dyn Error>) -> Self {
        Self::Other(error.to_string())
    }
}

impl From<CompileError> for BuildError {
    fn from(error: CompileError) -> Self {
        Self::Compile(error)
    }
}

impl From<io::Error> for BuildError {
    fn from(error: io::Error) -> Self {
        Self::Other(error.to_string())
    }
}

impl From<JoinError> for BuildError {
    fn from(error: JoinError) -> Self {
        Self::Other(error.to_string())
    }
}

impl From<ModuleDependencyError> for BuildError {
    fn from(error: ModuleDependencyError) -> Self {
        Self::ModuleDependency(error)
    }
}

impl From<ParseError> for BuildError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl From<BuildGraphError> for BuildError {
    fn from(error: BuildGraphError) -> Self {
        Self::BuildGraph(error)
    }
}
