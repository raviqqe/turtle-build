use super::module_state::ModuleState;
use crate::ir::Build;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct GlobalState<'a> {
    pub outputs: HashMap<String, Arc<Build>>,
    pub default_outputs: HashSet<String>,
    pub module: ModuleState<'a>,
}
