#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    name: String,
    command: String,
    description: String,
}

impl Rule {
    pub fn new(
        name: impl Into<String>,
        command: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            description: description.into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}
