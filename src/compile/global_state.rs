use crate::ir::Build;
use fnv::{FnvHashMap, FnvHashSet};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct GlobalState<'a> {
    pub outputs: FnvHashMap<&'a str, Arc<Build<'a>>>,
    pub default_outputs: FnvHashSet<&'a str>,
    pub source_map: FnvHashMap<&'a str, &'a str>,
}
