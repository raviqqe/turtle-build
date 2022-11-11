use crate::{ast::Module, context::Context};
use std::{collections::HashMap, path::PathBuf};

pub type ModuleDependencyMap = HashMap<PathBuf, HashMap<String, PathBuf>>;

#[derive(Debug, Default)]
pub struct Context<'c, 'a> {
    application: &Context,
    modules: HashMap<PathBuf, Module<'a>>,
    dependencies: ModuleDependencyMap,
}

impl<'a> Context<'a> {
    pub fn new(
        application: &Context,
        modules: HashMap<PathBuf, Module<'a>>,
        dependencies: ModuleDependencyMap,
    ) -> Self {
        Self {
            application,
            modules,
            dependencies,
        }
    }

    pub fn application(&self) -> Foo {}

    pub fn modules(&self) -> &HashMap<PathBuf, Module<'a>> {
        &self.modules
    }

    pub fn dependencies(&self) -> &ModuleDependencyMap {
        &self.dependencies
    }
}
