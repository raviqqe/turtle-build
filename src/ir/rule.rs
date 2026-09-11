#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Dependency {
    Depfile { path: String },
    Gcc { path: String },
    Msvc { prefix: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    command: String,
    description: Option<String>,
    dependency: Option<Dependency>,
}

impl Rule {
    pub fn new(command: impl Into<String>, description: Option<String>) -> Self {
        Self {
            command: command.into(),
            description,
            dependency: None,
        }
    }

    pub fn with_dependency(mut self, dependency: Option<Dependency>) -> Self {
        self.dependency = dependency;
        self
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn dependency(&self) -> Option<&Dependency> {
        self.dependency.as_ref()
    }
}
