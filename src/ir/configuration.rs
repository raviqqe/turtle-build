use super::Build;
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration {
    outputs: HashMap<String, Arc<Build>>,
}

impl Configuration {
    pub fn new(outputs: HashMap<String, Arc<Build>>) -> Self {
        Self { outputs }
    }

    pub fn outputs(&self) -> &HashMap<String, Arc<Build>> {
        &self.outputs
    }
}
