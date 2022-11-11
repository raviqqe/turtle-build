use super::DynamicBuild;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicConfiguration {
    outputs: HashMap<SmolStr, DynamicBuild>,
}

impl DynamicConfiguration {
    pub fn new(outputs: HashMap<SmolStr, DynamicBuild>) -> Self {
        Self { outputs }
    }

    pub fn outputs(&self) -> &HashMap<SmolStr, DynamicBuild> {
        &self.outputs
    }
}
