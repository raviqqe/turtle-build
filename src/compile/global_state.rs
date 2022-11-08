use crate::ir::Build;
use fnv::{FnvHashMap, FnvHashSet};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct GlobalState<'a> {
    pub outputs: FnvHashMap<String, Arc<Build<'a>>>,
    pub default_outputs: FnvHashSet<String>,
    pub source_map: FnvHashMap<String, String>,
}
