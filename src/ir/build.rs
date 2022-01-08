use super::Rule;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    id: String,
    rule: Arc<Rule>,
    inputs: Vec<String>,
}

impl Build {
    pub fn new(id: impl Into<String>, rule: Arc<Rule>, inputs: Vec<String>) -> Self {
        Self {
            id: id.into(),
            rule,
            inputs,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn rule(&self) -> &Arc<Rule> {
        &self.rule
    }

    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }
}
