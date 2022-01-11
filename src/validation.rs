mod error;

use self::error::ValidationError;
use crate::ir::{Build, Configuration};
use petgraph::{algo::toposort, Graph};
use std::{collections::HashMap, sync::Arc};

pub fn validate_configuration(configuration: &Configuration) -> Result<(), ValidationError> {
    if is_output_dependency_circular(&configuration.outputs()) {
        return Err(ValidationError::CircularOutputDependency);
    }

    Ok(())
}

fn is_output_dependency_circular(dependencies: &HashMap<String, Arc<Build>>) -> bool {
    let mut graph = Graph::<&str, ()>::new();
    let mut indices = HashMap::<&str, _>::new();

    for output in dependencies.keys() {
        indices.insert(&output, graph.add_node(&output));
    }

    for (output, build) in dependencies {
        for input in build.inputs().iter().chain(build.order_only_inputs()) {
            graph.add_edge(indices[output.as_str()], indices[input.as_str()], ());
        }
    }

    toposort(&graph, None).is_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Rule;

    mod configuration {
        use super::*;

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
        fn validate_single_build() {
            assert_eq!(
                validate_configuration(&Configuration::new(
                    [(
                        "foo".into(),
                        Build::new("", Rule::new("", "").into(), vec![], vec![]).into()
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
                        Build::new("", Rule::new("", "").into(), vec!["foo".into()], vec![]).into()
                    )]
                    .into_iter()
                    .collect(),
                    Default::default(),
                    None,
                )),
                Err(ValidationError::CircularOutputDependency)
            );
        }

        #[test]
        fn validate_circular_build_with_order_only_input() {
            assert_eq!(
                validate_configuration(&Configuration::new(
                    [(
                        "foo".into(),
                        Build::new("", Rule::new("", "").into(), vec![], vec!["foo".into()]).into()
                    )]
                    .into_iter()
                    .collect(),
                    Default::default(),
                    None,
                )),
                Err(ValidationError::CircularOutputDependency)
            );
        }

        #[test]
        fn validate_two_builds() {
            assert_eq!(
                validate_configuration(&Configuration::new(
                    [
                        (
                            "foo".into(),
                            Build::new("", Rule::new("", "").into(), vec!["bar".into()], vec![])
                                .into()
                        ),
                        (
                            "bar".into(),
                            Build::new("", Rule::new("", "").into(), vec![], vec![]).into()
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
                            Build::new("", Rule::new("", "").into(), vec!["bar".into()], vec![])
                                .into()
                        ),
                        (
                            "bar".into(),
                            Build::new("", Rule::new("", "").into(), vec!["foo".into()], vec![])
                                .into()
                        )
                    ]
                    .into_iter()
                    .collect(),
                    Default::default(),
                    None,
                )),
                Err(ValidationError::CircularOutputDependency)
            );
        }
    }
}
