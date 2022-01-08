use super::{Build, Rule, Submodule, VariableDefinition};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    variable_definitions: Vec<VariableDefinition>,
    rules: Vec<Rule>,
    builds: Vec<Build>,
    submodules: Vec<Submodule>,
}

impl Module {
    pub fn new(
        variable_definitions: Vec<VariableDefinition>,
        rules: Vec<Rule>,
        builds: Vec<Build>,
        submodules: Vec<Submodule>,
    ) -> Self {
        Self {
            variable_definitions,
            rules,
            builds,
            submodules,
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

    pub fn submodules(&self) -> &[Submodule] {
        &self.submodules
    }
}
