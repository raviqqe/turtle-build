#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicBuild {
    output: String,
    implicit_inputs: Vec<String>,
}

impl DynamicBuild {
    pub fn new(output: impl Into<String>, implicit_inputs: Vec<String>) -> Self {
        Self {
            output: output.into(),
            implicit_inputs,
        }
    }

    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn implicit_inputs(&self) -> &[String] {
        &self.implicit_inputs
    }
}
