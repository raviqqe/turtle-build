use crate::{ast::Module, path_pool::PathPool};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug)]
pub struct CompileContext<'a> {
    modules: &'a HashMap<PathBuf, Module>,
    path_pool: &'a PathPool,
}

impl<'a> CompileContext<'a> {
    pub const fn new(modules: &'a HashMap<PathBuf, Module>, path_pool: &'a PathPool) -> Self {
        Self { modules, path_pool }
    }

    pub const fn modules(&self) -> &HashMap<PathBuf, Module> {
        self.modules
    }

    pub const fn path_pool(&self) -> &PathPool {
        self.path_pool
    }
}
