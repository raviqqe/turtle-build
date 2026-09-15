use crate::ir::{Build, Pool};
use alloc::sync::Arc;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct GlobalState {
    pub outputs: HashMap<Arc<str>, Arc<Build>>,
    pub default_outputs: HashSet<Arc<str>>,
    pub source_map: HashMap<Arc<str>, Arc<str>>,
    // A pool of `None` has no limit.
    pub pools: HashMap<Arc<str>, Option<Pool>>,
}
