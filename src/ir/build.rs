#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    outputs: Vec<String>,
    rule: String,
    inputs: Vec<String>,
}

impl Build {
    pub fn new(outputs: Vec<String>, rule: impl Into<String>, inputs: Vec<String>) -> Self {
        Self {
            outputs,
            rule: rule.into(),
            inputs,
        }
    }

    pub fn outputs(&self) -> &[String] {
        &self.outputs
    }

    pub fn rule(&self) -> &str {
        &self.rule
    }

    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }
}
