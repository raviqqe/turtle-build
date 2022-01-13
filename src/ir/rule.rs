#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    command: String,
    description: Option<String>,
}

impl Rule {
    pub fn new(command: impl Into<String>, description: Option<String>) -> Self {
        Self {
            command: command.into(),
            description,
        }
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}
