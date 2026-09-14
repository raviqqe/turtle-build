use crate::ir::Build;
use alloc::sync::Arc;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct GlobalState {
    pub outputs: HashMap<Arc<str>, Arc<Build>>,
    pub default_outputs: HashSet<Arc<str>>,
    pub source_map: HashMap<Arc<str>, Arc<str>>,
}
