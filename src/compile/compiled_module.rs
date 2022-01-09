use crate::{ast, ir::Build};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct CompiledModule {
    pub outputs: HashMap<String, Arc<Build>>,
    pub default_outputs: HashSet<String>,
    pub rules: HashMap<String, ast::Rule>,
    pub variables: HashMap<String, String>,
}
