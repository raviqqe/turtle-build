use crate::{ast::Module, module_dependency_map::ModuleDependencyMap};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Default)]
pub struct Context {
    modules: HashMap<PathBuf, Module>,
    dependencies: ModuleDependencyMap,
}

impl Context {
    pub fn new(modules: HashMap<PathBuf, Module>, dependencies: ModuleDependencyMap) -> Self {
        Self {
            modules,
            dependencies,
        }
    }

    pub fn modules(&self) -> &HashMap<PathBuf, Module> {
        &self.modules
    }

    pub fn dependencies(&self) -> &ModuleDependencyMap {
        &self.dependencies
    }
}
