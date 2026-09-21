use crate::{ast::Module, module_dependency::ModuleDependencyMap, path_pool::PathPool};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug)]
pub struct CompileContext<'a> {
    modules: &'a HashMap<PathBuf, Module>,
    dependencies: &'a ModuleDependencyMap,
    path_pool: &'a PathPool,
}

impl<'a> CompileContext<'a> {
    pub const fn new(
        modules: &'a HashMap<PathBuf, Module>,
        dependencies: &'a ModuleDependencyMap,
        path_pool: &'a PathPool,
    ) -> Self {
        Self {
            modules,
            dependencies,
            path_pool,
        }
    }

    pub const fn modules(&self) -> &HashMap<PathBuf, Module> {
        self.modules
    }

    pub const fn dependencies(&self) -> &ModuleDependencyMap {
        self.dependencies
    }

    pub const fn path_pool(&self) -> &PathPool {
        self.path_pool
    }
}
