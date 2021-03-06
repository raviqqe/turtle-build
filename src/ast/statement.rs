use super::{Build, DefaultOutput, Include, Rule, Submodule, VariableDefinition};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement {
    Build(Build),
    Default(DefaultOutput),
    Include(Include),
    Rule(Rule),
    Submodule(Submodule),
    VariableDefinition(VariableDefinition),
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

impl From<Include> for Statement {
    fn from(include: Include) -> Self {
        Self::Include(include)
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
