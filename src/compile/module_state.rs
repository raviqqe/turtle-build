use crate::ast;
use std::sync::Arc;
use train_map::TrainMap;

#[derive(Clone, Debug)]
pub struct ModuleState<'a, 'm> {
    pub rules: TrainMap<'m, &'a str, ast::Rule>,
    pub variables: TrainMap<'m, &'a str, Arc<str>>,
}

impl<'a, 'm> ModuleState<'a, 'm> {
    pub fn fork(&'m self) -> Self {
        Self {
            rules: self.rules.fork(),
            variables: self.variables.fork(),
        }
    }
}
