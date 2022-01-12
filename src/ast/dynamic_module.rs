use super::DynamicBuild;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicModule {
    builds: Vec<DynamicBuild>,
}

impl DynamicModule {
    pub fn new(builds: Vec<DynamicBuild>) -> Self {
        Self { builds }
    }

    pub fn builds(&self) -> &[DynamicBuild] {
        &self.builds
    }
}
