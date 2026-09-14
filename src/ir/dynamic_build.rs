use alloc::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicBuild {
    inputs: Vec<Arc<str>>,
}

impl DynamicBuild {
    pub const fn new(inputs: Vec<Arc<str>>) -> Self {
        Self { inputs }
    }

    pub fn inputs(&self) -> &[Arc<str>] {
        &self.inputs
    }
}
