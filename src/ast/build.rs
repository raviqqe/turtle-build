use super::VariableDefinition;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build<'a> {
    outputs: Vec<&'a str>,
    implicit_outputs: Vec<&'a str>,
    rule: &'a str,
    inputs: Vec<&'a str>,
    implicit_inputs: Vec<&'a str>,
    order_only_inputs: Vec<&'a str>,
    variable_definitions: Vec<VariableDefinition<'a>>,
}

impl<'a> Build<'a> {
    pub fn new(
        outputs: Vec<&'a str>,
        implicit_outputs: Vec<&'a str>,
        rule: &'a str,
        inputs: Vec<&'a str>,
        implicit_inputs: Vec<&'a str>,
        order_only_inputs: Vec<&'a str>,
        variable_definitions: Vec<VariableDefinition<'a>>,
    ) -> Self {
        Self {
            outputs,
            implicit_outputs,
            rule,
            inputs,
            implicit_inputs,
            order_only_inputs,
            variable_definitions,
        }
    }

    pub fn outputs(&self) -> &[&str] {
        &self.outputs
    }

    pub fn implicit_outputs(&self) -> &[&str] {
        &self.implicit_outputs
    }

    pub fn rule(&self) -> &str {
        self.rule
    }

    pub fn inputs(&self) -> &[&str] {
        &self.inputs
    }

    pub fn implicit_inputs(&self) -> &[&str] {
        &self.implicit_inputs
    }

    pub fn order_only_inputs(&self) -> &[&'a str] {
        &self.order_only_inputs
    }

    pub fn variable_definitions(&self) -> &[VariableDefinition] {
        &self.variable_definitions
    }
}
