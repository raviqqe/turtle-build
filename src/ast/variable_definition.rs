#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariableDefinition {
    name: String,
    value: String,
}

impl VariableDefinition {
    pub const fn new(name: String, value: String) -> Self {
        Self { name, value }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}
