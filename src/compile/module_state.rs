use super::chain_map::ChainMap;
use crate::ast;

#[derive(Clone, Debug)]
pub struct ModuleState<'a, 'b> {
    pub rules: ChainMap<'b, String, ast::Rule<'a>>,
    pub variables: ChainMap<'b, String, String>,
}

impl<'a, 'b> ModuleState<'a, 'b> {
    pub fn fork(&'b self) -> Self {
        Self {
            rules: self.rules.fork(),
            variables: self.variables.fork(),
        }
    }
}
