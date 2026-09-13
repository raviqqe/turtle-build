#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum HeaderDependency {
    Make { path: String },
    Gcc { path: String },
    Msvc { prefix: String },
}
