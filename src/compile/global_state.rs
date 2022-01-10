use crate::ir::Build;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct GlobalState {
    pub outputs: HashMap<String, Arc<Build>>,
    pub default_outputs: HashSet<String>,
}
