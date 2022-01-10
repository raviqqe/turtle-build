use super::VariableDefinition;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    outputs: Vec<String>,
    implicit_outputs: Vec<String>,
    rule: String,
    inputs: Vec<String>,
    implicit_inputs: Vec<String>,
    variable_definitions: Vec<VariableDefinition>,
}

impl Build {
    pub fn new(
        outputs: Vec<String>,
        implicit_outputs: Vec<String>,
        rule: impl Into<String>,
        inputs: Vec<String>,
        implicit_inputs: Vec<String>,
        variable_definitions: Vec<VariableDefinition>,
    ) -> Self {
        Self {
            outputs,
            implicit_outputs,
            rule: rule.into(),
            inputs,
            implicit_inputs,
            variable_definitions,
        }
    }

    pub fn outputs(&self) -> &[String] {
        &self.outputs
    }

    pub fn implicit_outputs(&self) -> &[String] {
        &self.implicit_outputs
    }

    pub fn rule(&self) -> &str {
        &self.rule
    }

    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }

    pub fn implicit_inputs(&self) -> &[String] {
        &self.implicit_inputs
    }

    pub fn variable_definitions(&self) -> &[VariableDefinition] {
        &self.variable_definitions
    }
}
