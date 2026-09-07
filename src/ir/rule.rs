#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum DependencyStyle {
    Gcc,
    Msvc { prefix: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    command: String,
    description: Option<String>,
    depfile: Option<String>,
    dependency_style: Option<DependencyStyle>,
}

impl Rule {
    pub fn new(command: impl Into<String>, description: Option<String>) -> Self {
        Self {
            command: command.into(),
            description,
            depfile: None,
            dependency_style: None,
        }
    }

    pub fn with_depfile(mut self, depfile: Option<String>) -> Self {
        self.depfile = depfile;
        self
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

    pub fn depfile(&self) -> Option<&str> {
        self.depfile.as_deref()
    }

    pub fn dependency_style(&self) -> Option<&DependencyStyle> {
        self.dependency_style.as_ref()
    }
}
