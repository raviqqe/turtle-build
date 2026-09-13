use super::VariableDefinition;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    name: String,
    variable_definitions: Vec<VariableDefinition>,
}

impl Rule {
    pub fn new(name: impl Into<String>, variable_definitions: Vec<VariableDefinition>) -> Self {
        Self {
            name: name.into(),
            variable_definitions,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn variable_definitions(&self) -> &[VariableDefinition] {
        &self.variable_definitions
    }
}
