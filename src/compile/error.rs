use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    path::PathBuf,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompileError {
    ModuleNotFound(PathBuf),
    RuleNotFound(String),
}

impl Error for CompileError {}

impl Display for CompileError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::ModuleNotFound(path) => {
                write!(formatter, "module \"{}\" not found", path.display())
            }
            Self::RuleNotFound(rule) => {
                write!(formatter, "rule \"{rule}\" not found")
            }
        }
    }
}
