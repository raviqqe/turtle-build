use crate::{ast::Module, module_dependency::ModuleDependencyMap};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug)]
pub struct Context<'a> {
    modules: &'a HashMap<PathBuf, Module>,
    dependencies: &'a ModuleDependencyMap,
}

impl<'a> Context<'a> {
    pub fn new(
        modules: &'a HashMap<PathBuf, Module>,
        dependencies: &'a ModuleDependencyMap,
    ) -> Self {
        Self {
            modules,
            dependencies,
        }
    }

    pub fn modules(&self) -> &HashMap<PathBuf, Module> {
        self.modules
    }

    pub fn dependencies(&self) -> &ModuleDependencyMap {
        self.dependencies
    }
}
