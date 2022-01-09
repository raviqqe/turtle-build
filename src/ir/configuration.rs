use super::Build;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration {
    outputs: HashMap<String, Arc<Build>>,
    default_outputs: HashSet<String>,
}

impl Configuration {
    pub fn new(outputs: HashMap<String, Arc<Build>>, default_outputs: HashSet<String>) -> Self {
        Self {
            outputs,
            default_outputs,
        }
    }

    pub fn outputs(&self) -> &HashMap<String, Arc<Build>> {
        &self.outputs
    }

    pub fn default_outputs(&self) -> &HashSet<String> {
        &self.default_outputs
    }
}
