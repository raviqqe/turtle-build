#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    command: SmolStr,
    description: Option<SmolStr>,
}

impl Rule {
    pub fn new(command: impl Into<SmolStr>, description: Option<SmolStr>) -> Self {
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
