use crate::{
    build_graph::BuildGraphError,
    compile::CompileError,
    infrastructure::{CommandError, ConsoleError, DatabaseError, FileError},
    ir::Build,
    module_dependency::ModuleDependencyError,
    parse::ParseError,
};
use alloc::sync::Arc;
use core::{
    error::Error,
    fmt::{self, Display, Formatter},
};
use tokio::{io, sync::AcquireError, task::JoinError};

/// A build error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildError {
    /// A semaphore acquisition error.
    Acquire(String),
    /// A build failure.
    Build,
    /// A build graph error.
    BuildGraph(BuildGraphError),
    /// A command error.
    Command(CommandError),
    /// A compile error.
    Compile(CompileError),
    /// A console error.
    Console(ConsoleError),
    /// A database error.
    Database(DatabaseError),
    /// A default output not found.
    DefaultOutputNotFound(Arc<str>),
    /// A dynamic dependency not found.
    DynamicDependencyNotFound(Arc<Build>),
    /// A file error.
    File(FileError),
    /// A file not found.
    FileNotFound(String),
    /// An input not built.
    InputNotBuilt(String),
    /// An input not found.
    InputNotFound(String),
    /// An I/O error.
    Io(String),
    /// A task join error.
    Join(String),
    /// A module dependency error.
    ModuleDependency(ModuleDependencyError),
    /// An output not found.
    OutputNotFound(String),
    /// A parse error.
    Parse(ParseError),
}

impl Error for BuildError {}

impl Display for BuildError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::Acquire(message) | Self::Io(message) | Self::Join(message) => {
                write!(formatter, "{message}")
            }
            Self::Build => write!(formatter, "build failed"),
            Self::Command(error) => write!(formatter, "{error}"),
            Self::Compile(error) => write!(formatter, "{error}"),
            Self::Console(error) => write!(formatter, "{error}"),
            Self::Database(error) => write!(formatter, "{error}"),
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
            Self::File(error) => write!(formatter, "{error}"),
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
            Self::OutputNotFound(output) => {
                write!(formatter, "output \"{output}\" not found")
            }
            Self::Parse(error) => write!(formatter, "{error}"),
            Self::BuildGraph(error) => write!(formatter, "{error}"),
        }
    }
}

impl From<AcquireError> for BuildError {
    fn from(error: AcquireError) -> Self {
        Self::Acquire(error.to_string())
    }
}

impl From<CommandError> for BuildError {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}

impl From<CompileError> for BuildError {
    fn from(error: CompileError) -> Self {
        Self::Compile(error)
    }
}

impl From<ConsoleError> for BuildError {
    fn from(error: ConsoleError) -> Self {
        Self::Console(error)
    }
}

impl From<DatabaseError> for BuildError {
    fn from(error: DatabaseError) -> Self {
        Self::Database(error)
    }
}

impl From<FileError> for BuildError {
    fn from(error: FileError) -> Self {
        Self::File(error)
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
