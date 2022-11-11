use super::DynamicBuild;
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicConfiguration {
    outputs: HashMap<Arc<str>, DynamicBuild>,
}

impl DynamicConfiguration {
    pub fn new(outputs: HashMap<Arc<str>, DynamicBuild>) -> Self {
        Self { outputs }
    }

    pub fn outputs(&self) -> &HashMap<Arc<str>, DynamicBuild> {
        &self.outputs
    }
}
