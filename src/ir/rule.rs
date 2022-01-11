#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    command: String,
    description: String,
}

impl Rule {
    pub fn new(command: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            description: description.into(),
        }
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    #[allow(dead_code)]
    pub fn description(&self) -> &str {
        &self.description
    }
}
