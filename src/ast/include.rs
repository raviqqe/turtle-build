#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Include {
    outputs: Vec<String>,
}

impl Include {
    pub fn new(outputs: Vec<String>) -> Self {
        Self { outputs }
    }

    pub fn outputs(&self) -> &[String] {
        &self.outputs
    }
}
