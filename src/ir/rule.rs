#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyStyle {
    Gcc,
    Msvc,
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

    pub fn with_dependencies(
        command: impl Into<String>,
        description: Option<String>,
        depfile: Option<String>,
        dependency_style: Option<DependencyStyle>,
    ) -> Self {
        Self {
            command: command.into(),
            description,
            depfile,
            dependency_style,
        }
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

    pub fn dependency_style(&self) -> Option<DependencyStyle> {
        self.dependency_style
    }
}
