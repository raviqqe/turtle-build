use super::Build;
use fnv::{FnvHashMap, FnvHashSet};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration<'a> {
    outputs: FnvHashMap<SmolStr, Arc<Build<'a>>>,
    default_outputs: FnvHashSet<SmolStr>,
    source_map: FnvHashMap<SmolStr, SmolStr>,
    build_directory: Option<SmolStr>,
}

impl<'a> Configuration<'a> {
    pub fn new(
        outputs: FnvHashMap<SmolStr, Arc<Build<'a>>>,
        default_outputs: FnvHashSet<SmolStr>,
        source_map: FnvHashMap<SmolStr, SmolStr>,
        build_directory: Option<SmolStr>,
    ) -> Self {
        Self {
            outputs,
            default_outputs,
            source_map,
            build_directory,
        }
    }

    pub fn outputs(&self) -> &FnvHashMap<SmolStr, Arc<Build<'a>>> {
        &self.outputs
    }

    pub fn default_outputs(&self) -> &FnvHashSet<SmolStr> {
        &self.default_outputs
    }

    pub fn source_map(&self) -> &FnvHashMap<SmolStr, SmolStr> {
        &self.source_map
    }

    pub fn build_directory(&self) -> Option<&str> {
        self.build_directory.as_deref()
    }
}
