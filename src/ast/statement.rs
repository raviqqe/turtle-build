use super::{Build, DefaultOutput, Include, Rule, Submodule, VariableDefinition};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement<'a> {
    Build(Build<'a>),
    Default(DefaultOutput<'a>),
    Include(Include<'a>),
    Rule(Rule<'a>),
    Submodule(Submodule<'a>),
    VariableDefinition(VariableDefinition<'a>),
}

impl<'a> From<Build<'a>> for Statement<'a> {
    fn from(build: Build<'a>) -> Self {
        Self::Build(build)
    }
}

impl<'a> From<DefaultOutput<'a>> for Statement<'a> {
    fn from(default: DefaultOutput<'a>) -> Self {
        Self::Default(default)
    }
}

impl<'a> From<Include<'a>> for Statement<'a> {
    fn from(include: Include<'a>) -> Self {
        Self::Include(include)
    }
}

impl<'a> From<Rule<'a>> for Statement<'a> {
    fn from(rule: Rule<'a>) -> Self {
        Self::Rule(rule)
    }
}

impl<'a> From<Submodule<'a>> for Statement<'a> {
    fn from(submodule: Submodule<'a>) -> Self {
        Self::Submodule(submodule)
    }
}

impl<'a> From<VariableDefinition<'a>> for Statement<'a> {
    fn from(variable_definition: VariableDefinition<'a>) -> Self {
        Self::VariableDefinition(variable_definition)
    }
}
