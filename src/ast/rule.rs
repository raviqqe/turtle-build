#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    name: String,
    command: String,
    description: Option<String>,
    depfile: Option<String>,
    deps: Option<String>,
    msvc_deps_prefix: Option<String>,
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
            msvc_deps_prefix: None,
        }
    }

    pub fn with_depfile(mut self, depfile: Option<String>) -> Self {
        self.depfile = depfile;
        self
    }

    pub fn with_deps(mut self, deps: Option<String>) -> Self {
        self.deps = deps;
        self
    }

    pub fn with_msvc_deps_prefix(mut self, msvc_deps_prefix: Option<String>) -> Self {
        self.msvc_deps_prefix = msvc_deps_prefix;
        self
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

    pub fn msvc_deps_prefix(&self) -> Option<&str> {
        self.msvc_deps_prefix.as_deref()
    }
}
