#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum HeaderDependency {
    Depfile { path: String },
    Gcc { path: String },
    Msvc { prefix: String },
}
