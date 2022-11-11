use super::Build;
use fnv::{FnvHashMap, FnvHashSet};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration {
    outputs: FnvHashMap<Arc<str>, Arc<Build>>,
    default_outputs: FnvHashSet<Arc<str>>,
    source_map: FnvHashMap<Arc<str>, Arc<str>>,
    build_directory: Option<Arc<str>>,
}

impl Configuration {
    pub fn new(
        outputs: FnvHashMap<Arc<str>, Arc<Build>>,
        default_outputs: FnvHashSet<Arc<str>>,
        source_map: FnvHashMap<Arc<str>, Arc<str>>,
        build_directory: Option<Arc<str>>,
    ) -> Self {
        Self {
            outputs,
            default_outputs,
            source_map,
            build_directory,
        }
    }

    pub fn outputs(&self) -> &FnvHashMap<Arc<str>, Arc<Build>> {
        &self.outputs
    }

    pub fn default_outputs(&self) -> &FnvHashSet<Arc<str>> {
        &self.default_outputs
    }

    pub fn source_map(&self) -> &FnvHashMap<Arc<str>, Arc<str>> {
        &self.source_map
    }

    pub fn build_directory(&self) -> Option<&Arc<str>> {
        self.build_directory.as_ref()
    }
}
