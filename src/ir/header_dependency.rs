use crate::path_pool::FilePath;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum HeaderDependency {
    Make { path: FilePath },
    Gcc { path: FilePath },
    Msvc { prefix: String },
}
