use crate::module_dependency_map::ModuleDependencyMap;
use petgraph::{algo::is_cyclic_directed, Graph};
use std::{collections::HashMap, path::Path};

use super::ValidationError;

pub fn validate_modules(modules: &ModuleDependencyMap) -> Result<(), ValidationError> {
    if is_module_dependency_circular(modules) {
        return Err(ValidationError::CircularModuleDependency);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_empty() {
        assert_eq!(validate_modules(&Default::default()), Ok(()));
    }

    #[test]
    fn validate_module() {
        assert_eq!(
            validate_modules(&[("foo".into(), Default::default())].into_iter().collect()),
            Ok(())
        );
    }

    #[test]
    fn validate_circular_module() {
        assert_eq!(
            validate_modules(
                &[(
                    "foo".into(),
                    [("foo".into(), "foo".into())].into_iter().collect()
                )]
                .into_iter()
                .collect()
            ),
            Err(ValidationError::CircularModuleDependency)
        );
    }

    #[test]
    fn validate_two_modules() {
        assert_eq!(
            validate_modules(
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
            validate_modules(
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
            Err(ValidationError::CircularModuleDependency)
        );
    }
}
