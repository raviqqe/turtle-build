use super::{Rule, VariableDefinition};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    variable_definitions: Vec<VariableDefinition>,
    rules: Vec<Rule>,
}

impl Module {
    pub fn new(variable_definitions: Vec<VariableDefinition>, rules: Vec<Rule>) -> Self {
        Self {
            variable_definitions,
            rules,
        }
    }

    pub fn variable_definitions(&self) -> &[VariableDefinition] {
        &self.variable_definitions
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }
}
