use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicBuild {
    inputs: Vec<Arc<str>>,
}

impl DynamicBuild {
    pub fn new(inputs: Vec<Arc<str>>) -> Self {
        Self { inputs }
    }

    pub fn inputs(&self) -> &[Arc<str>] {
        &self.inputs
    }
}
