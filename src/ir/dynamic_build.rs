#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicBuild {
    inputs: Vec<SmolStr>,
}

impl DynamicBuild {
    pub fn new(inputs: Vec<SmolStr>) -> Self {
        Self { inputs }
    }

    pub fn inputs(&self) -> &[SmolStr] {
        &self.inputs
    }
}
