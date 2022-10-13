#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    command: String,
    description: Option<String>,
    always: bool,
}

impl Rule {
    pub fn new(command: impl Into<String>, description: Option<String>, always: bool) -> Self {
        Self {
            command: command.into(),
            description,
            always,
        }
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn always(&self) -> bool {
        self.always
    }
}
