use super::{DynamicBuild, PathId};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicConfiguration {
    outputs: HashMap<PathId, DynamicBuild>,
}

impl DynamicConfiguration {
    pub fn new(outputs: HashMap<PathId, DynamicBuild>) -> Self {
        Self { outputs }
    }

    pub fn outputs(&self) -> &HashMap<PathId, DynamicBuild> {
        &self.outputs
    }
}
