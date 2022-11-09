use crate::ir::Build;
use fnv::{FnvHashMap, FnvHashSet};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct GlobalState {
    pub outputs: FnvHashMap<String, Arc<Build>>,
    pub default_outputs: FnvHashSet<String>,
    pub source_map: FnvHashMap<String, String>,
}
