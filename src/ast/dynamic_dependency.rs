use super::Build;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicDependency {
    version: String,
    builds: Vec<Build>,
}

impl DynamicDependency {
    pub fn new(version: impl Into<String>, builds: Vec<Build>) -> Self {
        Self {
            version: version.into(),
            builds,
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn builds(&self) -> &[Build] {
        &self.builds
    }
}
