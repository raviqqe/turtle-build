use super::{Build, Rule, VariableDefinition};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    variable_definitions: Vec<VariableDefinition>,
    rules: Vec<Rule>,
    builds: Vec<Build>,
}

impl Module {
    pub fn new(
        variable_definitions: Vec<VariableDefinition>,
        rules: Vec<Rule>,
        builds: Vec<Build>,
    ) -> Self {
        Self {
            variable_definitions,
            rules,
            builds,
        }
    }

    pub fn variable_definitions(&self) -> &[VariableDefinition] {
        &self.variable_definitions
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn builds(&self) -> &[Build] {
        &self.builds
    }
}
