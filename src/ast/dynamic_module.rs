use super::DynamicBuild;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicModule {
    version: String,
    builds: Vec<DynamicBuild>,
}

impl DynamicModule {
    pub fn new(version: impl Into<String>, builds: Vec<DynamicBuild>) -> Self {
        Self {
            version: version.into(),
            builds,
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn builds(&self) -> &[DynamicBuild] {
        &self.builds
    }
}
