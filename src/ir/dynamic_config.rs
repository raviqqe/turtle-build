use super::DynamicBuild;
use alloc::sync::Arc;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicConfig {
    outputs: HashMap<Arc<str>, DynamicBuild>,
}

impl DynamicConfig {
    pub const fn new(outputs: HashMap<Arc<str>, DynamicBuild>) -> Self {
        Self { outputs }
    }

    pub const fn outputs(&self) -> &HashMap<Arc<str>, DynamicBuild> {
        &self.outputs
    }
}
