use std::{collections::HashMap, path::PathBuf};

/// A module dependency map.
pub type ModuleDependencyMap = HashMap<PathBuf, HashMap<String, PathBuf>>;
