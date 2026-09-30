use super::DynamicBuild;
use alloc::sync::Arc;
use rapidhash::RapidHashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicConfig {
    outputs: RapidHashMap<Arc<str>, DynamicBuild>,
}

impl DynamicConfig {
    pub const fn new(outputs: RapidHashMap<Arc<str>, DynamicBuild>) -> Self {
        Self { outputs }
    }

    pub const fn outputs(&self) -> &RapidHashMap<Arc<str>, DynamicBuild> {
        &self.outputs
    }
}
