use super::Build;
use alloc::sync::Arc;
use core::num::NonZeroUsize;
use rapidhash::{RapidHashMap, RapidHashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    outputs: RapidHashMap<Arc<str>, Arc<Build>>,
    default_outputs: RapidHashSet<Arc<str>>,
    source_map: RapidHashMap<Arc<str>, Arc<str>>,
    pools: RapidHashMap<Arc<str>, NonZeroUsize>,
    build_directory: Option<Arc<str>>,
}

impl Config {
    pub const fn new(
        outputs: RapidHashMap<Arc<str>, Arc<Build>>,
        default_outputs: RapidHashSet<Arc<str>>,
        source_map: RapidHashMap<Arc<str>, Arc<str>>,
        pools: RapidHashMap<Arc<str>, NonZeroUsize>,
        build_directory: Option<Arc<str>>,
    ) -> Self {
        Self {
            outputs,
            default_outputs,
            source_map,
            pools,
            build_directory,
        }
    }

    pub const fn outputs(&self) -> &RapidHashMap<Arc<str>, Arc<Build>> {
        &self.outputs
    }

    pub const fn default_outputs(&self) -> &RapidHashSet<Arc<str>> {
        &self.default_outputs
    }

    pub const fn source_map(&self) -> &RapidHashMap<Arc<str>, Arc<str>> {
        &self.source_map
    }

    pub const fn pools(&self) -> &RapidHashMap<Arc<str>, NonZeroUsize> {
        &self.pools
    }

    pub const fn build_directory(&self) -> Option<&Arc<str>> {
        self.build_directory.as_ref()
    }
}
