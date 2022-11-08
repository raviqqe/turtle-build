use super::{Build, DefaultOutput, Include, Rule, Submodule, VariableDefinition};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement<'a> {
    Build(Build<'a>),
    Default(DefaultOutput<'a>),
    Include(Include),
    Rule(Rule<'a>),
    Submodule(Submodule<'a>),
    VariableDefinition(VariableDefinition<'a>),
}

impl<'a> From<Build<'a>> for Statement<'a> {
    fn from(build: Build) -> Self {
        Self::Build(build)
    }
}

impl<'a> From<DefaultOutput<'a>> for Statement<'a> {
    fn from(default: DefaultOutput) -> Self {
        Self::Default(default)
    }
}

impl<'a> From<Include<'a>> for Statement<'a> {
    fn from(include: Include) -> Self {
        Self::Include(include)
    }
}

impl<'a> From<Rule<'a>> for Statement<'a> {
    fn from(rule: Rule) -> Self {
        Self::Rule(rule)
    }
}

impl<'a> From<Submodule<'a>> for Statement<'a> {
    fn from(submodule: Submodule) -> Self {
        Self::Submodule(submodule)
    }
}

impl<'a> From<VariableDefinition<'a>> for Statement<'a> {
    fn from(variable_definition: VariableDefinition) -> Self {
        Self::VariableDefinition(variable_definition)
    }
}
