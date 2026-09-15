use core::{
    error::Error,
    fmt::{self, Display, Formatter},
};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompileError {
    DuplicatePool(String),
    InvalidDependencyStyle(String),
    InvalidPoolDepth(String, String),
    MissingDepfile(String),
    ModuleNotFound(PathBuf),
    PoolNotFound(String),
    RuleNotFound(String),
}

impl Error for CompileError {}

impl Display for CompileError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::DuplicatePool(pool) => {
                write!(formatter, "pool \"{pool}\" already defined")
            }
            Self::InvalidDependencyStyle(style) => {
                write!(formatter, "dependency style \"{style}\" not supported")
            }
            Self::InvalidPoolDepth(pool, depth) => {
                write!(formatter, "pool \"{pool}\" has invalid depth \"{depth}\"")
            }
            Self::MissingDepfile(rule) => {
                write!(
                    formatter,
                    "rule \"{rule}\" has \"deps\" set to \"gcc\" but no \"depfile\""
                )
            }
            Self::ModuleNotFound(path) => {
                write!(formatter, "module \"{}\" not found", path.display())
            }
            Self::PoolNotFound(pool) => {
                write!(formatter, "pool \"{pool}\" not found")
            }
            Self::RuleNotFound(rule) => {
                write!(formatter, "rule \"{rule}\" not found")
            }
        }
    }
}
