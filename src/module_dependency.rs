use petgraph::{algo::is_cyclic_directed, Graph};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::Path;
use std::{collections::HashMap, path::PathBuf};

pub type ModuleDependencyMap = HashMap<PathBuf, HashMap<String, PathBuf>>;

pub fn validate(modules: &ModuleDependencyMap) -> Result<(), ModuleDependencyError> {
    if is_module_dependency_circular(modules) {
        return Err(ModuleDependencyError::CircularDependency);
    }

    Ok(())
}

fn is_module_dependency_circular(modules: &ModuleDependencyMap) -> bool {
    let mut graph = Graph::<&Path, ()>::new();
    let mut indices = HashMap::<&Path, _>::new();

    for output in modules.keys() {
        indices.insert(output, graph.add_node(output));
    }

    for (output, build) in modules {
        for input in build.values() {
            graph.add_edge(indices[output.as_path()], indices[input.as_path()], ());
        }
    }

    is_cyclic_directed(&graph)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleDependencyError {
    CircularDependency,
}

impl Error for ModuleDependencyError {}

impl Display for ModuleDependencyError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::CircularDependency => {
                write!(formatter, "build file dependency cycle detected")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_empty() {
        assert_eq!(validate(&Default::default()), Ok(()));
    }

    #[test]
    fn validate_module() {
        assert_eq!(
            validate(&[("foo".into(), Default::default())].into_iter().collect()),
            Ok(())
        );
    }

    #[test]
    fn validate_circular_module() {
        assert_eq!(
            validate(
                &[(
                    "foo".into(),
                    [("foo".into(), "foo".into())].into_iter().collect()
                )]
                .into_iter()
                .collect()
            ),
            Err(ModuleDependencyError::CircularDependency)
        );
    }

    #[test]
    fn validate_two_modules() {
        assert_eq!(
            validate(
                &[
                    (
                        "foo".into(),
                        [("bar".into(), "bar".into())].into_iter().collect()
                    ),
                    ("bar".into(), Default::default(),)
                ]
                .into_iter()
                .collect()
            ),
            Ok(())
        );
    }

    #[test]
    fn validate_two_circular_modules() {
        assert_eq!(
            validate(
                &[
                    (
                        "foo".into(),
                        [("bar".into(), "bar".into())].into_iter().collect()
                    ),
                    (
                        "bar".into(),
                        [("foo".into(), "foo".into())].into_iter().collect()
                    ),
                ]
                .into_iter()
                .collect()
            ),
            Err(ModuleDependencyError::CircularDependency)
        );
    }
}
