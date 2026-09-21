use crate::path_pool::FilePath;
use core::{
    hash::{Hash, Hasher},
    mem::discriminant,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeaderDependency {
    Make { path: FilePath },
    Gcc { path: FilePath },
    Msvc { prefix: String },
}

// Hashes depend only on path strings because they are persistent.
impl Hash for HeaderDependency {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        discriminant(self).hash(hasher);

        match self {
            Self::Make { path } | Self::Gcc { path } => path.as_str().hash(hasher),
            Self::Msvc { prefix } => prefix.hash(hasher),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_pool::PathPool;
    use core::hash::{BuildHasher, BuildHasherDefault};
    use pretty_assertions::assert_eq;
    use std::collections::hash_map::DefaultHasher;

    #[derive(Hash)]
    enum StringHeaderDependency {
        Make { path: &'static str },
        Gcc { path: &'static str },
        Msvc { prefix: &'static str },
    }

    fn hash(value: impl Hash) -> u64 {
        BuildHasherDefault::<DefaultHasher>::default().hash_one(value)
    }

    #[test]
    fn keep_hash_format() {
        let path_pool = PathPool::new();

        for (dependency, string_dependency) in [
            (
                HeaderDependency::Make {
                    path: path_pool.intern("foo"),
                },
                StringHeaderDependency::Make { path: "foo" },
            ),
            (
                HeaderDependency::Gcc {
                    path: path_pool.intern("foo"),
                },
                StringHeaderDependency::Gcc { path: "foo" },
            ),
            (
                HeaderDependency::Msvc {
                    prefix: "foo".into(),
                },
                StringHeaderDependency::Msvc { prefix: "foo" },
            ),
        ] {
            assert_eq!(hash(dependency), hash(string_dependency));
        }
    }

    #[test]
    fn keep_hash_across_path_pools() {
        assert_eq!(
            hash(HeaderDependency::Gcc {
                path: PathPool::new().intern("foo"),
            }),
            hash(HeaderDependency::Gcc {
                path: PathPool::new().intern("foo"),
            })
        );
    }
}
