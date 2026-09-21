use crate::{ast::Module, module_dependency::ModuleDependencyMap};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug)]
pub struct CompileContext<'a> {
    modules: &'a HashMap<PathBuf, Module>,
    dependencies: &'a ModuleDependencyMap,
}

impl<'a> CompileContext<'a> {
    pub const fn new(
        modules: &'a HashMap<PathBuf, Module>,
        dependencies: &'a ModuleDependencyMap,
    ) -> Self {
        Self {
            modules,
            dependencies,
        }
    }

    pub const fn modules(&self) -> &HashMap<PathBuf, Module> {
        self.modules
    }

    pub const fn dependencies(&self) -> &ModuleDependencyMap {
        self.dependencies
    }
}
