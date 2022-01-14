use super::error::ValidationError;
use crate::ir::{Build, DynamicConfiguration};
use petgraph::{
    algo::is_cyclic_directed,
    graph::{DefaultIx, NodeIndex},
    Graph,
};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug)]
pub struct BuildGraph {
    graph: Graph<String, ()>,
    indexes: HashMap<String, NodeIndex<DefaultIx>>,
}

impl BuildGraph {
    pub fn new(outputs: &HashMap<String, Arc<Build>>) -> Self {
        let mut graph = Graph::<String, ()>::new();
        let mut indexes = HashMap::<String, NodeIndex<DefaultIx>>::new();

        for output in outputs.iter().flat_map(|(output, build)| {
            [output]
                .into_iter()
                .chain(build.inputs().iter().chain(build.order_only_inputs()))
        }) {
            indexes.insert(output.clone(), graph.add_node(output.clone()));
        }

        for (output, build) in outputs {
            for input in build.inputs().iter().chain(build.order_only_inputs()) {
                graph.add_edge(indexes[output.as_str()], indexes[input.as_str()], ());
            }
        }

        Self { graph, indexes }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if is_cyclic_directed(&self.graph) {
            return Err(ValidationError::CircularBuildDependency);
        }

        Ok(())
    }

    pub fn insert(&mut self, configuration: &DynamicConfiguration) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Rule;

    fn validate_builds(dependencies: &HashMap<String, Arc<Build>>) -> Result<(), ValidationError> {
        BuildGraph::new(dependencies).validate()
    }

    fn ir_explicit_build(id: impl Into<String>, rule: Rule, inputs: Vec<String>) -> Build {
        Build::new(id, vec![], vec![], rule.into(), inputs, vec![], None)
    }

    #[test]
    fn validate_empty() {
        assert_eq!(validate_builds(&Default::default()), Ok(()));
    }

    #[test]
    fn validate_build_without_input() {
        assert_eq!(
            validate_builds(
                &[(
                    "foo".into(),
                    ir_explicit_build("", Rule::new("", None), vec![]).into()
                )]
                .into_iter()
                .collect()
            ),
            Ok(())
        );
    }

    #[test]
    fn validate_build_with_explicit_input() {
        assert_eq!(
            validate_builds(
                &[(
                    "foo".into(),
                    ir_explicit_build("", Rule::new("", None), vec!["bar".into()]).into()
                )]
                .into_iter()
                .collect()
            ),
            Ok(())
        );
    }

    #[test]
    fn validate_build_with_order_only_input() {
        assert_eq!(
            validate_builds(
                &[(
                    "foo".into(),
                    Build::new(
                        "",
                        vec![],
                        vec![],
                        Rule::new("", None).into(),
                        vec![],
                        vec!["bar".into()],
                        None
                    )
                    .into()
                )]
                .into_iter()
                .collect()
            ),
            Ok(())
        );
    }

    #[test]
    fn validate_circular_build_with_explicit_input() {
        assert_eq!(
            validate_builds(
                &[(
                    "foo".into(),
                    ir_explicit_build("", Rule::new("", None), vec!["foo".into()]).into()
                )]
                .into_iter()
                .collect()
            ),
            Err(ValidationError::CircularBuildDependency)
        );
    }

    #[test]
    fn validate_circular_build_with_order_only_input() {
        assert_eq!(
            validate_builds(
                &[(
                    "foo".into(),
                    Build::new(
                        "",
                        vec![],
                        vec![],
                        Rule::new("", None).into(),
                        vec![],
                        vec!["foo".into()],
                        None
                    )
                    .into()
                )]
                .into_iter()
                .collect()
            ),
            Err(ValidationError::CircularBuildDependency)
        );
    }

    #[test]
    fn validate_two_builds() {
        assert_eq!(
            validate_builds(
                &[
                    (
                        "foo".into(),
                        ir_explicit_build("", Rule::new("", None), vec!["bar".into()]).into()
                    ),
                    (
                        "bar".into(),
                        ir_explicit_build("", Rule::new("", None), vec![]).into()
                    )
                ]
                .into_iter()
                .collect()
            ),
            Ok(())
        );
    }

    #[test]
    fn validate_two_circular_builds() {
        assert_eq!(
            validate_builds(
                &[
                    (
                        "foo".into(),
                        ir_explicit_build("", Rule::new("", None), vec!["bar".into()]).into()
                    ),
                    (
                        "bar".into(),
                        ir_explicit_build("", Rule::new("", None), vec!["foo".into()]).into()
                    )
                ]
                .into_iter()
                .collect()
            ),
            Err(ValidationError::CircularBuildDependency)
        );
    }
}
