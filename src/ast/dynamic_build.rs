#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicBuild {
    output: String,
    implicit_inputs: Vec<String>,
}

impl DynamicBuild {
    pub const fn new(output: String, implicit_inputs: Vec<String>) -> Self {
        Self {
            output,
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
