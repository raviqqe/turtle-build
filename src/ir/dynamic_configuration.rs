use super::DynamicBuild;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicConfiguration {
    outputs: HashMap<String, DynamicBuild>,
}

impl DynamicConfiguration {
    pub fn new(outputs: HashMap<String, DynamicBuild>) -> Self {
        Self { outputs }
    }

    pub fn outputs(&self) -> &HashMap<String, DynamicBuild> {
        &self.outputs
    }
}
