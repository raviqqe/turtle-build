#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    id: String,
    command: String,
    description: String,
    inputs: Vec<String>,
}

impl Build {
    pub fn new(
        id: impl Into<String>,
        command: impl Into<String>,
        description: impl Into<String>,
        inputs: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            command: command.into(),
            description: description.into(),
            inputs,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }
}