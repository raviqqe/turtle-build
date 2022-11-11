use super::chain_map::ChainMap;
use crate::ast;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ModuleState<'a, 'm> {
    pub rules: ChainMap<'m, &'a str, ast::Rule>,
    pub variables: ChainMap<'m, &'a str, Arc<str>>,
}

impl<'a, 'm> ModuleState<'a, 'm> {
    pub fn fork(&'m self) -> Self {
        Self {
            rules: self.rules.fork(),
            variables: self.variables.fork(),
        }
    }
}
