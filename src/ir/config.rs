use super::Build;
use crate::path_pool::FilePath;
use alloc::sync::Arc;
use core::num::NonZeroUsize;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    outputs: HashMap<FilePath, Arc<Build>>,
    default_outputs: HashSet<FilePath>,
    source_map: HashMap<FilePath, Arc<str>>,
    pools: HashMap<Arc<str>, NonZeroUsize>,
    build_directory: Option<Arc<str>>,
}

impl Config {
    pub const fn new(
        outputs: HashMap<FilePath, Arc<Build>>,
        default_outputs: HashSet<FilePath>,
        source_map: HashMap<FilePath, Arc<str>>,
        pools: HashMap<Arc<str>, NonZeroUsize>,
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

    pub const fn outputs(&self) -> &HashMap<FilePath, Arc<Build>> {
        &self.outputs
    }

    pub const fn default_outputs(&self) -> &HashSet<FilePath> {
        &self.default_outputs
    }

    pub const fn source_map(&self) -> &HashMap<FilePath, Arc<str>> {
        &self.source_map
    }

    pub const fn pools(&self) -> &HashMap<Arc<str>, NonZeroUsize> {
        &self.pools
    }

    pub const fn build_directory(&self) -> Option<&Arc<str>> {
        self.build_directory.as_ref()
    }
}
