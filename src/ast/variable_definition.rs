#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariableDefinition {
    name: String,
    value: String,
}

impl VariableDefinition {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}
