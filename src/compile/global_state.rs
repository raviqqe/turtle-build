use crate::ir::Build;
use fnv::FnvHashMap;
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Debug)]
pub struct GlobalState {
    pub outputs: FnvHashMap<String, Arc<Build>>,
    pub default_outputs: HashSet<String>,
}
