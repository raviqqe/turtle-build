use std::{collections::HashMap, path::PathBuf};

pub type ModuleDependencyMap = HashMap<PathBuf, HashMap<String, PathBuf>>;
