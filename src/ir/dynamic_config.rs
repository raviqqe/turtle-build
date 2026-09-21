use super::DynamicBuild;
use crate::path_pool::FilePath;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicConfig {
    outputs: HashMap<FilePath, DynamicBuild>,
}

impl DynamicConfig {
    pub const fn new(outputs: HashMap<FilePath, DynamicBuild>) -> Self {
        Self { outputs }
    }

    pub const fn outputs(&self) -> &HashMap<FilePath, DynamicBuild> {
        &self.outputs
    }
}
