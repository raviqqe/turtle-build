use crate::{ir::Build, path_pool::FilePath};
use alloc::sync::Arc;
use core::num::NonZeroUsize;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct GlobalState {
    pub outputs: HashMap<FilePath, Arc<Build>>,
    pub default_outputs: HashSet<FilePath>,
    pub source_map: HashMap<FilePath, Arc<str>>,
    pub pools: HashMap<Arc<str>, Option<NonZeroUsize>>,
}
