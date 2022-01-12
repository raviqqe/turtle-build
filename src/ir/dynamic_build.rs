#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicBuild {
    inputs: Vec<String>,
}

impl DynamicBuild {
    pub fn new(inputs: Vec<String>) -> Self {
        Self { inputs }
    }

    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }
}
