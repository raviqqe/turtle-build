use super::chain_map::ChainMap;
use crate::ast;

#[derive(Clone, Debug)]
pub struct ModuleState<'a> {
    pub rules: ChainMap<'a, String, ast::Rule>,
    pub variables: ChainMap<'a, String, String>,
}

impl<'a> ModuleState<'a> {
    pub fn derive(&'a self) -> Self {
        Self {
            rules: self.rules.derive(),
            variables: self.variables.derive(),
        }
    }
}
