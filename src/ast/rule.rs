#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    name: String,
    command: String,
    description: Option<String>,
    depfile: Option<String>,
    deps: Option<String>,
}

impl Rule {
    pub fn new(
        name: impl Into<String>,
        command: impl Into<String>,
        description: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            description,
            depfile: None,
            deps: None,
        }
    }

    pub fn with_dependencies(
        name: impl Into<String>,
        command: impl Into<String>,
        description: Option<String>,
        depfile: Option<String>,
        deps: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            description,
            depfile,
            deps,
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

    pub fn depfile(&self) -> Option<&str> {
        self.depfile.as_deref()
    }

    pub fn deps(&self) -> Option<&str> {
        self.deps.as_deref()
    }
}
