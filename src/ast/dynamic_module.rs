use super::DynamicBuild;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicModule<'a> {
    builds: Vec<DynamicBuild<'a>>,
}

impl<'a> DynamicModule<'a> {
    pub fn new(builds: Vec<DynamicBuild<'a>>) -> Self {
        Self { builds }
    }

    pub fn builds(&self) -> &[DynamicBuild<'a>] {
        &self.builds
    }
}
