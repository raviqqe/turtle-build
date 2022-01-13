mod error;

use self::error::ValidationError;
use crate::{
    compile::ModuleDependencyMap,
    ir::{Build, Configuration},
};
use petgraph::{algo::is_cyclic_directed, Graph};
use std::{collections::HashMap, path::Path, sync::Arc};

pub fn validate_configuration(configuration: &Configuration) -> Result<(), ValidationError> {
    if is_output_dependency_circular(configuration.outputs()) {
        return Err(ValidationError::CircularBuildDependency);
    }

    Ok(())
}

fn is_output_dependency_circular(dependencies: &HashMap<String, Arc<Build>>) -> bool {
    let mut graph = Graph::<&str, ()>::new();
    let mut indices = HashMap::<&str, _>::new();

    for output in dependencies.iter().flat_map(|(output, build)| {
        [output]
            .into_iter()
            .chain(build.inputs().iter().chain(build.order_only_inputs()))
    }) {
        indices.insert(output, graph.add_node(output));
    }

    for (output, build) in dependencies {
        for input in build.inputs().iter().chain(build.order_only_inputs()) {
            graph.add_edge(indices[output.as_str()], indices[input.as_str()], ());
        }
    }

    is_cyclic_directed(&graph)
}

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
    use crate::ir::Rule;

    mod outputs {
        use super::*;

        fn ir_explicit_build(id: impl Into<String>, rule: Rule, inputs: Vec<String>) -> Build {
            Build::new(id, vec![], rule.into(), inputs, vec![], None)
        }

        #[test]
        fn validate_empty() {
            assert_eq!(
                validate_configuration(&Configuration::new(
                    Default::default(),
                    Default::default(),
                    None,
                )),
                Ok(())
            );
        }

        #[test]
        fn validate_build_without_input() {
            assert_eq!(
                validate_configuration(&Configuration::new(
                    [(
                        "foo".into(),
                        ir_explicit_build("", Rule::new("", ""), vec![]).into()
                    )]
                    .into_iter()
                    .collect(),
                    Default::default(),
                    None,
                )),
                Ok(())
            );
        }

        #[test]
        fn validate_build_with_explicit_input() {
            assert_eq!(
                validate_configuration(&Configuration::new(
                    [(
                        "foo".into(),
                        ir_explicit_build("", Rule::new("", ""), vec!["bar".into()]).into()
                    )]
                    .into_iter()
                    .collect(),
                    Default::default(),
                    None,
                )),
                Ok(())
            );
        }

        #[test]
        fn validate_build_with_order_only_input() {
            assert_eq!(
                validate_configuration(&Configuration::new(
                    [(
                        "foo".into(),
                        Build::new(
                            "",
                            vec![],
                            Rule::new("", "").into(),
                            vec![],
                            vec!["bar".into()],
                            None
                        )
                        .into()
                    )]
                    .into_iter()
                    .collect(),
                    Default::default(),
                    None,
                )),
                Ok(())
            );
        }

        #[test]
        fn validate_circular_build_with_explicit_input() {
            assert_eq!(
                validate_configuration(&Configuration::new(
                    [(
                        "foo".into(),
                        ir_explicit_build("", Rule::new("", ""), vec!["foo".into()]).into()
                    )]
                    .into_iter()
                    .collect(),
                    Default::default(),
                    None,
                )),
                Err(ValidationError::CircularBuildDependency)
            );
        }

        #[test]
        fn validate_circular_build_with_order_only_input() {
            assert_eq!(
                validate_configuration(&Configuration::new(
                    [(
                        "foo".into(),
                        Build::new(
                            "",
                            vec![],
                            Rule::new("", "").into(),
                            vec![],
                            vec!["foo".into()],
                            None
                        )
                        .into()
                    )]
                    .into_iter()
                    .collect(),
                    Default::default(),
                    None,
                )),
                Err(ValidationError::CircularBuildDependency)
            );
        }

        #[test]
        fn validate_two_builds() {
            assert_eq!(
                validate_configuration(&Configuration::new(
                    [
                        (
                            "foo".into(),
                            ir_explicit_build("", Rule::new("", ""), vec!["bar".into()]).into()
                        ),
                        (
                            "bar".into(),
                            ir_explicit_build("", Rule::new("", ""), vec![]).into()
                        )
                    ]
                    .into_iter()
                    .collect(),
                    Default::default(),
                    None,
                )),
                Ok(())
            );
        }

        #[test]
        fn validate_two_circular_builds() {
            assert_eq!(
                validate_configuration(&Configuration::new(
                    [
                        (
                            "foo".into(),
                            ir_explicit_build("", Rule::new("", ""), vec!["bar".into()]).into()
                        ),
                        (
                            "bar".into(),
                            ir_explicit_build("", Rule::new("", ""), vec!["foo".into()]).into()
                        )
                    ]
                    .into_iter()
                    .collect(),
                    Default::default(),
                    None,
                )),
                Err(ValidationError::CircularBuildDependency)
            );
        }
    }

    mod modules {
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
}
