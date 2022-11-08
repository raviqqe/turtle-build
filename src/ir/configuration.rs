use super::Build;
use fnv::{FnvHashMap, FnvHashSet};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration<'a> {
    outputs: FnvHashMap<String, Arc<Build<'a>>>,
    default_outputs: FnvHashSet<String>,
    source_map: FnvHashMap<String, String>,
    build_directory: Option<String>,
}

impl<'a> Configuration<'a> {
    pub fn new(
        outputs: FnvHashMap<String, Arc<Build<'a>>>,
        default_outputs: FnvHashSet<String>,
        source_map: FnvHashMap<String, String>,
        build_directory: Option<String>,
    ) -> Self {
        Self {
            outputs,
            default_outputs,
            source_map,
            build_directory,
        }
    }

    pub fn outputs(&self) -> &FnvHashMap<String, Arc<Build<'a>>> {
        &self.outputs
    }

    pub fn default_outputs(&self) -> &FnvHashSet<String> {
        &self.default_outputs
    }

    pub fn source_map(&self) -> &FnvHashMap<String, String> {
        &self.source_map
    }

    pub fn build_directory(&self) -> Option<&str> {
        self.build_directory.as_deref()
    }
}
