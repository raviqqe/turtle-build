use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::PathBuf,
};

pub struct BuildIdCalculator {
    index: usize,
    module_path: PathBuf,
}

impl BuildIdCalculator {
    pub fn new(module_path: PathBuf) -> Self {
        Self {
            index: 0,
            module_path,
        }
    }

    pub fn calculate(&mut self) -> String {
        let mut hasher = DefaultHasher::new();

        self.module_path.hash(&mut hasher);
        self.index.hash(&mut hasher);

        self.index += 1;

        format!("{:x}", hasher.finish())
    }
}
