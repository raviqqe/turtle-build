#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum DependencyStyle {
    Depfile { path: String },
    Gcc { path: String },
    Msvc { prefix: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    command: String,
    description: Option<String>,
    dependency_style: Option<DependencyStyle>,
}

impl Rule {
    pub fn new(command: impl Into<String>, description: Option<String>) -> Self {
        Self {
            command: command.into(),
            description,
            dependency_style: None,
        }
    }

    pub fn with_dependency_style(mut self, dependency_style: Option<DependencyStyle>) -> Self {
        self.dependency_style = dependency_style;
        self
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn dependency_style(&self) -> Option<&DependencyStyle> {
        self.dependency_style.as_ref()
    }
}
