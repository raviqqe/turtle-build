use std::path::PathBuf;

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
        let id = format!("{}:{}", self.module_path.display(), self.index);

        self.index += 1;

        id
    }
}
