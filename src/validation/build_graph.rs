use super::error::ValidationError;
use crate::ir::{Build, DynamicConfiguration};
use petgraph::{
    algo::{kosaraju_scc, toposort},
    graph::{DefaultIx, NodeIndex},
    Graph,
};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug)]
pub struct BuildGraph {
    graph: Graph<String, ()>,
    nodes: HashMap<String, NodeIndex<DefaultIx>>,
}

impl BuildGraph {
    pub fn new(outputs: &HashMap<String, Arc<Build>>) -> Result<Self, ValidationError> {
        let mut this = Self {
            graph: Graph::<String, ()>::new(),
            nodes: HashMap::<String, NodeIndex<DefaultIx>>::new(),
        };

        for (output, build) in outputs {
            for input in build.inputs().iter().chain(build.order_only_inputs()) {
                this.add_node(output);
                this.add_node(input);

                this.add_edge(output, input);
            }
        }

        this.validate()?;

        Ok(this)
    }

    pub fn insert(&mut self, configuration: &DynamicConfiguration) -> Result<(), ValidationError> {
        for (output, build) in configuration.outputs() {
            for input in build.inputs() {
                self.add_node(output);
                self.add_node(input);

                self.add_edge(output, input);
            }
        }

        self.validate()
    }

    fn validate(&self) -> Result<(), ValidationError> {
        if let Err(cycle) = toposort(&self.graph, None) {
            let mut components = kosaraju_scc(&self.graph);

            components.sort_by_key(|component| component.len());

            return Err(ValidationError::CircularBuildDependency(
                components
                    .into_iter()
                    .rev()
                    .find(|component| component.contains(&cycle.node_id()))
                    .unwrap()
                    .into_iter()
                    .map(|id| self.graph[id].clone())
                    .collect(),
            ));
        }

        Ok(())
    }

    fn add_node(&mut self, output: &str) {
        if !self.nodes.contains_key(output) {
            self.nodes
                .insert(output.into(), self.graph.add_node(output.into()));
        }
    }

    fn add_edge(&mut self, output: &str, input: &str) {
        self.graph
            .add_edge(self.nodes[output], self.nodes[input], ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{DynamicBuild, Rule};

    fn validate_builds(dependencies: &HashMap<String, Arc<Build>>) -> Result<(), ValidationError> {
        BuildGraph::new(dependencies)?;

        Ok(())
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
            Err(ValidationError::CircularBuildDependency(vec!["foo".into()]))
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
            Err(ValidationError::CircularBuildDependency(vec!["foo".into()]))
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
            Err(ValidationError::CircularBuildDependency(vec![
                "bar".into(),
                "foo".into(),
            ]))
        );
    }

    #[test]
    fn validate_with_dynamic_configuration() {
        let mut graph = BuildGraph::new(
            &[(
                "foo".into(),
                ir_explicit_build("", Rule::new("", None), vec!["bar".into()]).into(),
            )]
            .into_iter()
            .collect(),
        )
        .unwrap();

        assert_eq!(
            graph.insert(&DynamicConfiguration::new(
                [("bar".into(), DynamicBuild::new(vec!["foo".into()]))]
                    .into_iter()
                    .collect(),
            )),
            Err(ValidationError::CircularBuildDependency(vec![
                "foo".into(),
                "bar".into(),
            ]))
        );
    }
}
