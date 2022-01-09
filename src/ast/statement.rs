use super::{Build, DefaultOutput, Rule, Submodule, VariableDefinition};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement {
    Build(Build),
    Default(DefaultOutput),
    Rule(Rule),
    Submodule(Submodule),
    VariableDefinition(VariableDefinition),
}

impl Statement {
    pub fn as_submodule(&self) -> Option<&Submodule> {
        match self {
            Self::Submodule(submodule) => Some(submodule),
            _ => None,
        }
    }
}

impl From<Build> for Statement {
    fn from(build: Build) -> Self {
        Self::Build(build)
    }
}

impl From<DefaultOutput> for Statement {
    fn from(default: DefaultOutput) -> Self {
        Self::Default(default)
    }
}

impl From<Rule> for Statement {
    fn from(rule: Rule) -> Self {
        Self::Rule(rule)
    }
}

impl From<Submodule> for Statement {
    fn from(submodule: Submodule) -> Self {
        Self::Submodule(submodule)
    }
}

impl From<VariableDefinition> for Statement {
    fn from(variable_definition: VariableDefinition) -> Self {
        Self::VariableDefinition(variable_definition)
    }
}
