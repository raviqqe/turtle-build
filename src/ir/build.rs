use super::Rule;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    rule: Arc<Rule>,
    inputs: Vec<String>,
}

impl Build {
    pub fn new(rule: Arc<Rule>, inputs: Vec<String>) -> Self {
        Self { rule, inputs }
    }

    pub fn rule(&self) -> &Rule {
        &self.rule
    }

    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }
}
