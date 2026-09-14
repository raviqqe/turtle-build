use super::Build;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    outputs: HashMap<Arc<str>, Arc<Build>>,
    default_outputs: HashSet<Arc<str>>,
    source_map: HashMap<Arc<str>, Arc<str>>,
    build_directory: Option<Arc<str>>,
}

impl Config {
    pub const fn new(
        outputs: HashMap<Arc<str>, Arc<Build>>,
        default_outputs: HashSet<Arc<str>>,
        source_map: HashMap<Arc<str>, Arc<str>>,
        build_directory: Option<Arc<str>>,
    ) -> Self {
        Self {
            outputs,
            default_outputs,
            source_map,
            build_directory,
        }
    }

    pub const fn outputs(&self) -> &HashMap<Arc<str>, Arc<Build>> {
        &self.outputs
    }

    pub const fn default_outputs(&self) -> &HashSet<Arc<str>> {
        &self.default_outputs
    }

    pub const fn source_map(&self) -> &HashMap<Arc<str>, Arc<str>> {
        &self.source_map
    }

    pub const fn build_directory(&self) -> Option<&Arc<str>> {
        self.build_directory.as_ref()
    }
}
