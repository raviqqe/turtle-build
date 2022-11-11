use super::chain_map::ChainMap;
use crate::ast;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ModuleState<'a, 'm> {
    pub rules: ChainMap<'m, String, ast::Rule<'a>>,
    pub variables: ChainMap<'m, String, Arc<str>>,
}

impl<'a, 'm> ModuleState<'a, 'm> {
    pub fn fork(&'m self) -> Self {
        Self {
            rules: self.rules.fork(),
            variables: self.variables.fork(),
        }
    }
}
