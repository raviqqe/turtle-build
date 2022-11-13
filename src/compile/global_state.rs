use crate::ir::Build;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct GlobalState {
    pub outputs: HashMap<Arc<str>, Arc<Build>>,
    pub default_outputs: HashSet<Arc<str>>,
    pub source_map: HashMap<Arc<str>, Arc<str>>,
}
