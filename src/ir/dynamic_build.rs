use crate::path_pool::FilePath;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicBuild {
    inputs: Vec<FilePath>,
}

impl DynamicBuild {
    pub const fn new(inputs: Vec<FilePath>) -> Self {
        Self { inputs }
    }

    pub fn inputs(&self) -> &[FilePath] {
        &self.inputs
    }
}
