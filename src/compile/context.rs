use crate::ast::Module;
use std::{collections::HashMap, path::PathBuf};

pub type ModuleDependencyMap = HashMap<PathBuf, HashMap<String, PathBuf>>;

#[derive(Debug, Default)]
pub struct Context<'a> {
    modules: HashMap<PathBuf, Module<'a>>,
    dependencies: ModuleDependencyMap,
}

impl<'a> Context<'a> {
    pub fn new(modules: HashMap<PathBuf, Module<'a>>, dependencies: ModuleDependencyMap) -> Self {
        Self {
            modules,
            dependencies,
        }
    }

    pub fn modules(&self) -> &HashMap<PathBuf, Module<'a>> {
        &self.modules
    }

    pub fn dependencies(&self) -> &ModuleDependencyMap {
        &self.dependencies
    }
}
