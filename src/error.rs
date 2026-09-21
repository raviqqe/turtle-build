use crate::{
    build_graph::BuildGraphError,
    compile::CompileError,
    infrastructure::{CommandError, ConsoleError, DatabaseError, FileError},
    ir::Build,
    module_dependency::ModuleDependencyError,
    parse::ParseError,
    path_pool::FilePath,
};
use alloc::sync::Arc;
use thiserror::Error;
use tokio::{io, sync::AcquireError, task::JoinError};

/// A build error.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum BuildError {
    /// A semaphore acquisition error.
    #[error("{0}")]
    Acquire(String),
    /// A build failure.
    #[error("build failed")]
    Build,
    /// A build graph error.
    #[error(transparent)]
    BuildGraph(#[from] BuildGraphError),
    /// A command error.
    #[error(transparent)]
    Command(#[from] CommandError),
    /// A compile error.
    #[error(transparent)]
    Compile(#[from] CompileError),
    /// A console error.
    #[error(transparent)]
    Console(#[from] ConsoleError),
    /// A database error.
    #[error(transparent)]
    Database(#[from] DatabaseError),
    /// A default output not found.
    #[error("default output \"{0}\" not found")]
    DefaultOutputNotFound(FilePath),
    /// A dynamic dependency not found.
    #[error(
        "outputs {} not found in dynamic dependency file {}",
        .0.outputs().join(", "),
        .0.dynamic_module().unwrap()
    )]
    DynamicDependencyNotFound(Arc<Build>),
    /// A file error.
    #[error(transparent)]
    File(#[from] FileError),
    /// A file not found.
    #[error("file \"{0}\" not found")]
    FileNotFound(String),
    /// An input not built.
    #[error("input \"{0}\" not built yet")]
    InputNotBuilt(String),
    /// An input not found.
    #[error("input \"{0}\" not found")]
    InputNotFound(String),
    /// An I/O error.
    #[error("{0}")]
    Io(String),
    /// A task join error.
    #[error("{0}")]
    Join(String),
    /// A module dependency error.
    #[error(transparent)]
    ModuleDependency(#[from] ModuleDependencyError),
    /// An output not found.
    #[error("output \"{0}\" not found")]
    OutputNotFound(String),
    /// A parse error.
    #[error(transparent)]
    Parse(#[from] ParseError),
}

impl From<AcquireError> for BuildError {
    fn from(error: AcquireError) -> Self {
        Self::Acquire(error.to_string())
    }
}

impl From<io::Error> for BuildError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<JoinError> for BuildError {
    fn from(error: JoinError) -> Self {
        Self::Join(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_acquire() {
        assert_eq!(
            BuildError::Acquire("semaphore closed".into()).to_string(),
            "semaphore closed"
        );
    }

    #[test]
    fn display_dynamic_dependency_not_found() {
        assert_eq!(
            BuildError::DynamicDependencyNotFound(
                Build::new(
                    vec!["foo".into(), "bar".into()],
                    vec![],
                    None,
                    vec![],
                    vec![],
                    Some("baz".into())
                )
                .into()
            )
            .to_string(),
            "outputs foo, bar not found in dynamic dependency file baz"
        );
    }

    #[test]
    fn display_io() {
        assert_eq!(
            BuildError::Io("permission denied".into()).to_string(),
            "permission denied"
        );
    }

    #[test]
    fn display_join() {
        assert_eq!(
            BuildError::Join("task cancelled".into()).to_string(),
            "task cancelled"
        );
    }
}
