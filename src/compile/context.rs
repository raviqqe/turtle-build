use crate::ast::Module;
use std::{cell::RefCell, collections::HashMap, path::PathBuf};

pub type ModuleDependencyMap = HashMap<PathBuf, HashMap<String, PathBuf>>;

#[derive(Debug, Default)]
pub struct Context {
    modules: HashMap<PathBuf, Module>,
    dependencies: ModuleDependencyMap,
    build_index: RefCell<usize>,
}

impl Context {
    pub fn new(modules: HashMap<PathBuf, Module>, dependencies: ModuleDependencyMap) -> Self {
        Self {
            modules,
            dependencies,
            build_index: RefCell::new(0),
        }
    }

    pub fn modules(&self) -> &HashMap<PathBuf, Module> {
        &self.modules
    }

    pub fn dependencies(&self) -> &ModuleDependencyMap {
        &self.dependencies
    }

    pub fn generate_build_id(&self) -> String {
        let index = *self.build_index.borrow();

        *self.build_index.borrow_mut() += 1;

        index.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_build_ids() {
        let context = Context::new(Default::default(), Default::default());

        assert_eq!(context.generate_build_id(), "0".to_string());
        assert_eq!(context.generate_build_id(), "1".to_string());
        assert_eq!(context.generate_build_id(), "2".to_string());
    }
}
