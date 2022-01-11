use super::Rule;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    id: String,
    rule: Option<Rule>,
    inputs: Vec<String>,
    order_only_inputs: Vec<String>,
}

impl Build {
    pub fn new(
        id: impl Into<String>,
        rule: Option<Rule>,
        inputs: Vec<String>,
        order_only_inputs: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            rule,
            inputs,
            order_only_inputs,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn rule(&self) -> Option<&Rule> {
        self.rule.as_ref()
    }

    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }

    pub fn order_only_inputs(&self) -> &[String] {
        &self.order_only_inputs
    }
}
