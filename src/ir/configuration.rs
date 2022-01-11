use super::Build;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration {
    outputs: HashMap<String, Arc<Build>>,
    default_outputs: HashSet<String>,
    build_directory: Option<String>,
}

impl Configuration {
    pub fn new(
        outputs: HashMap<String, Arc<Build>>,
        default_outputs: HashSet<String>,
        build_directory: Option<String>,
    ) -> Self {
        Self {
            outputs,
            default_outputs,
            build_directory,
        }
    }

    pub fn outputs(&self) -> &HashMap<String, Arc<Build>> {
        &self.outputs
    }

    pub fn default_outputs(&self) -> &HashSet<String> {
        &self.default_outputs
    }

    pub fn build_directory(&self) -> Option<&str> {
        self.build_directory.as_deref()
    }
}
