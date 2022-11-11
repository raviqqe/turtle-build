use crate::ir::Build;
use fnv::{FnvHashMap, FnvHashSet};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct GlobalState {
    pub outputs: FnvHashMap<Arc<str>, Arc<Build>>,
    pub default_outputs: FnvHashSet<Arc<str>>,
    pub source_map: FnvHashMap<Arc<str>, Arc<str>>,
}
