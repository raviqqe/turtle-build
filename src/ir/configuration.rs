use super::Build;
use fnv::{FnvHashMap, FnvHashSet};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration<'a> {
    outputs: FnvHashMap<&'a str, Arc<Build<'a>>>,
    default_outputs: FnvHashSet<&'a str>,
    source_map: FnvHashMap<&'a str, &'a str>,
    build_directory: Option<&'a str>,
}

impl<'a> Configuration<'a> {
    pub fn new(
        outputs: FnvHashMap<&'a str, Arc<Build<'a>>>,
        default_outputs: FnvHashSet<&'a str>,
        source_map: FnvHashMap<&'a str, &'a str>,
        build_directory: Option<&'a str>,
    ) -> Self {
        Self {
            outputs,
            default_outputs,
            source_map,
            build_directory,
        }
    }

    pub fn outputs(&self) -> &FnvHashMap<&'a str, Arc<Build>> {
        &self.outputs
    }

    pub fn default_outputs(&self) -> &FnvHashSet<&'a str> {
        &self.default_outputs
    }

    pub fn source_map(&self) -> &FnvHashMap<&'a str, &'a str> {
        &self.source_map
    }

    pub fn build_directory(&self) -> Option<&'a str> {
        self.build_directory.as_deref()
    }
}
