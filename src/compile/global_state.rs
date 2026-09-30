use crate::ir::Build;
use alloc::sync::Arc;
use core::num::NonZeroUsize;
use rapidhash::{RapidHashMap, RapidHashSet};

#[derive(Clone, Debug)]
pub struct GlobalState {
    pub outputs: RapidHashMap<Arc<str>, Arc<Build>>,
    pub default_outputs: RapidHashSet<Arc<str>>,
    pub source_map: RapidHashMap<Arc<str>, Arc<str>>,
    pub pools: RapidHashMap<Arc<str>, Option<NonZeroUsize>>,
}
