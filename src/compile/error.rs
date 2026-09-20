use std::path::PathBuf;
use thiserror::Error;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CompileError {
    #[error("pool \"{0}\" already defined")]
    DuplicatePool(String),
    #[error("dynamic dependency file \"{0}\" is not an input of its build")]
    DynamicModuleNotInput(String),
    #[error("dependency style \"{0}\" not supported")]
    InvalidDependencyStyle(String),
    #[error("pool \"{0}\" has invalid depth \"{1}\"")]
    InvalidPoolDepth(String, String),
    #[error("rule \"{0}\" has \"deps\" set to \"gcc\" but no \"depfile\"")]
    MissingDepfile(String),
    #[error("module \"{}\" not found", .0.display())]
    ModuleNotFound(PathBuf),
    #[error("pool \"{0}\" not found")]
    PoolNotFound(String),
    #[error("rule \"{0}\" not found")]
    RuleNotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_module_not_found() {
        assert_eq!(
            CompileError::ModuleNotFound("foo.ninja".into()).to_string(),
            "module \"foo.ninja\" not found"
        );
    }
}
