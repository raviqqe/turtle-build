use super::{HeaderDependency, Pool};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    command: String,
    description: Option<String>,
    header_dependency: Option<HeaderDependency>,
    pool: Option<Pool>,
}

impl Rule {
    pub fn new(command: impl Into<String>, description: Option<String>) -> Self {
        Self {
            command: command.into(),
            description,
            header_dependency: None,
            pool: None,
        }
    }

    pub fn with_header_dependency(mut self, header_dependency: Option<HeaderDependency>) -> Self {
        self.header_dependency = header_dependency;
        self
    }

    pub fn with_pool(mut self, pool: Option<Pool>) -> Self {
        self.pool = pool;
        self
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub const fn header_dependency(&self) -> Option<&HeaderDependency> {
        self.header_dependency.as_ref()
    }

    pub const fn pool(&self) -> Option<&Pool> {
        self.pool.as_ref()
    }
}
