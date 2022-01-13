use super::Rule;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    id: String,
    outputs: Vec<String>,
    rule: Option<Rule>,
    inputs: Vec<String>,
    order_only_inputs: Vec<String>,
    dynamic_module: Option<String>,
}

impl Build {
    pub fn new(
        id: impl Into<String>,
        outputs: Vec<String>,
        rule: Option<Rule>,
        inputs: Vec<String>,
        order_only_inputs: Vec<String>,
        dynamic_module: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            outputs,
            rule,
            inputs,
            order_only_inputs,
            dynamic_module,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn outputs(&self) -> &[String] {
        &self.outputs
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

    pub fn dynamic_module(&self) -> Option<&str> {
        self.dynamic_module.as_deref()
    }
}
