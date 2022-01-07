use super::VariableDefinition;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    variable_definitions: Vec<VariableDefinition>,
}

impl Module {
    pub fn new(variable_definitions: Vec<VariableDefinition>) -> Self {
        Self {
            variable_definitions,
        }
    }

    pub fn variable_definitions(&self) -> &[VariableDefinition] {
        &self.variable_definitions
    }
}
