use super::{Build, PathSet};
use fnv::{FnvHashMap, FnvHashSet};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration<'a> {
    outputs: FnvHashMap<String, Arc<Build>>,
    default_outputs: FnvHashSet<String>,
    source_map: FnvHashMap<String, String>,
    build_directory: Option<String>,
    paths: PathSet<'a>,
}

impl<'a> Configuration<'a> {
    pub fn new(
        outputs: FnvHashMap<String, Arc<Build>>,
        default_outputs: FnvHashSet<String>,
        source_map: FnvHashMap<String, String>,
        build_directory: Option<String>,
        paths: PathSet<'a>,
    ) -> Self {
        Self {
            outputs,
            default_outputs,
            source_map,
            build_directory,
            paths,
        }
    }

    pub fn outputs(&self) -> &FnvHashMap<String, Arc<Build>> {
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

    pub fn paths(&self) -> &PathSet<'a> {
        &self.paths
    }
}
