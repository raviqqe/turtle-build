#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultOutput {
    outputs: Vec<String>,
}

impl DefaultOutput {
    pub fn new(outputs: Vec<String>) -> Self {
        Self { outputs }
    }

    pub fn outputs(&self) -> &[String] {
        &self.outputs
    }
}
