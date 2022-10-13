#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    name: String,
    command: String,
    description: Option<String>,
    always: bool,
}

impl Rule {
    pub fn new(
        name: impl Into<String>,
        command: impl Into<String>,
        description: Option<String>,
        always: bool,
    ) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            description,
            always,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
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
