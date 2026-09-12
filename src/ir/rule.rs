use super::HeaderDependency;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    command: String,
    description: Option<String>,
    header_dependency: Option<HeaderDependency>,
}

impl Rule {
    pub fn new(command: impl Into<String>, description: Option<String>) -> Self {
        Self {
            command: command.into(),
            description,
            header_dependency: None,
        }
    }

    pub fn with_header_dependency(mut self, header_dependency: Option<HeaderDependency>) -> Self {
        self.header_dependency = header_dependency;
        self
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn header_dependency(&self) -> Option<&HeaderDependency> {
        self.header_dependency.as_ref()
    }
}
