use crate::ir::{Build, DynamicConfig};
use alloc::sync::Arc;
use core::{
    error::Error,
    fmt::{self, Display, Formatter},
};
use itertools::Itertools;
use petgraph::{
    Graph,
    algo::{kosaraju_scc, toposort},
    graph::{DefaultIx, NodeIndex},
};
use std::collections::HashMap;

#[derive(Debug)]
pub struct BuildGraph {
    graph: Graph<Arc<str>, ()>,
    nodes: HashMap<Arc<str>, NodeIndex<DefaultIx>>,
    primary_outputs: HashMap<Arc<str>, Arc<str>>,
}

impl BuildGraph {
    pub fn new(outputs: &HashMap<Arc<str>, Arc<Build>>) -> Self {
        let mut this = Self {
            graph: Graph::<Arc<str>, ()>::new(),
            nodes: HashMap::<Arc<str>, NodeIndex<DefaultIx>>::new(),
            primary_outputs: HashMap::new(),
        };

        for (output, build) in outputs {
            for input in build.inputs().iter().chain(build.order_only_inputs()) {
                this.add_edge(output.clone(), input.clone());
            }

            // Is this output primary?
            if output == &build.outputs()[0] {
                this.primary_outputs.insert(output.clone(), output.clone());

                for secondary in build
                    .outputs()
                    .iter()
                    .skip(1)
                    .chain(build.implicit_outputs())
                {
                    this.add_edge(secondary.clone(), output.clone());
                    this.primary_outputs
                        .insert(secondary.clone(), output.clone());
                }
            }
        }

        this
    }

    pub fn validate(&self) -> Result<(), BuildGraphError> {
        if let Err(cycle) = toposort(&self.graph, None) {
            let mut components = kosaraju_scc(&self.graph);

            components.sort_by_key(|component| component.len());

            return Err(BuildGraphError::CircularDependency(
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

    pub fn validate_dynamic(&mut self, config: &DynamicConfig) -> Result<(), BuildGraphError> {
        for (output, build) in config.outputs() {
            let output = self
                .primary_outputs
                .get(output)
                .ok_or_else(|| BuildGraphError::OutputNotFound(output.clone()))?
                .clone();

            for input in build.inputs() {
                self.add_edge(output.clone(), input.clone());
            }
        }

        self.validate()
    }

    pub fn add_header_dependencies(&mut self, output: &Arc<str>, dependencies: &[String]) {
        for dependency in dependencies {
            self.add_edge(
                self.primary_outputs[output].clone(),
                dependency.as_str().into(),
            );
        }
    }

    fn add_edge(&mut self, output: Arc<str>, input: Arc<str>) {
        self.add_node(&output);
        self.add_node(&input);

        self.graph
            .add_edge(self.nodes[&output], self.nodes[&input], ());
    }

    fn add_node(&mut self, output: &Arc<str>) {
        if !self.nodes.contains_key(output) {
            self.nodes
                .insert(output.clone(), self.graph.add_node(output.clone()));
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildGraphError {
    CircularDependency(Vec<Arc<str>>),
    OutputNotFound(Arc<str>),
}

impl Error for BuildGraphError {}

impl Display for BuildGraphError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::CircularDependency(cycle) => {
                write!(
                    formatter,
                    "dependency cycle detected: {}",
                    cycle
                        .iter()
                        .chain(cycle.first())
                        .dedup()
                        .map(|string| string.as_ref())
                        .collect::<Vec<&str>>()
                        .join(" -> ")
                )
            }
            Self::OutputNotFound(output) => write!(formatter, "output \"{output}\" not found"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{DynamicBuild, Rule};

    fn validate_builds(
        dependencies: &HashMap<Arc<str>, Arc<Build>>,
    ) -> Result<(), BuildGraphError> {
        BuildGraph::new(dependencies).validate()
    }

    fn explicit_build(outputs: Vec<Arc<str>>, inputs: Vec<Arc<str>>) -> Build {
        Build::new(
            outputs,
            vec![],
            Rule::new("", None).into(),
            inputs,
            vec![],
            None,
        )
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
                    explicit_build(vec!["foo".into()], vec![]).into()
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
                    explicit_build(vec!["foo".into()], vec!["bar".into()]).into()
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
                        vec!["foo".into()],
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
                    explicit_build(vec!["foo".into()], vec!["foo".into()]).into()
                )]
                .into_iter()
                .collect()
            ),
            Err(BuildGraphError::CircularDependency(vec!["foo".into()]))
        );
    }

    #[test]
    fn validate_circular_build_with_order_only_input() {
        assert_eq!(
            validate_builds(
                &[(
                    "foo".into(),
                    Build::new(
                        vec!["foo".into()],
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
            Err(BuildGraphError::CircularDependency(vec!["foo".into()]))
        );
    }

    #[test]
    fn validate_two_builds() {
        assert_eq!(
            validate_builds(
                &[
                    (
                        "foo".into(),
                        explicit_build(vec!["foo".into()], vec!["bar".into()]).into()
                    ),
                    (
                        "bar".into(),
                        explicit_build(vec!["bar".into()], vec![]).into()
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
        let BuildGraphError::CircularDependency(paths) = validate_builds(
            &[
                (
                    "foo".into(),
                    explicit_build(vec!["foo".into()], vec!["bar".into()]).into(),
                ),
                (
                    "bar".into(),
                    explicit_build(vec!["bar".into()], vec!["foo".into()]).into(),
                ),
            ]
            .into_iter()
            .collect(),
        )
        .unwrap_err() else {
            panic!()
        };

        assert_eq!(
            &paths,
            &if &*paths[0] == "foo" {
                ["foo".into(), "bar".into()]
            } else {
                ["bar".into(), "foo".into()]
            }
        );
    }

    #[test]
    fn validate_with_dynamic_config() {
        let mut graph = BuildGraph::new(
            &[
                (
                    "foo".into(),
                    explicit_build(vec!["foo".into()], vec!["bar".into()]).into(),
                ),
                (
                    "bar".into(),
                    explicit_build(vec!["bar".into()], vec![]).into(),
                ),
            ]
            .into_iter()
            .collect(),
        );

        graph.validate().unwrap();

        assert_eq!(
            graph.validate_dynamic(&DynamicConfig::new(
                [("bar".into(), DynamicBuild::new(vec!["foo".into()]))]
                    .into_iter()
                    .collect(),
            )),
            Err(BuildGraphError::CircularDependency(vec![
                "foo".into(),
                "bar".into(),
            ]))
        );
    }

    #[test]
    fn validate_with_dynamic_config_for_unknown_output() {
        let mut graph = BuildGraph::new(
            &[(
                "foo".into(),
                explicit_build(vec!["foo".into()], vec![]).into(),
            )]
            .into_iter()
            .collect(),
        );

        graph.validate().unwrap();

        assert_eq!(
            graph.validate_dynamic(&DynamicConfig::new(
                [("bar".into(), DynamicBuild::new(vec!["baz".into()]))]
                    .into_iter()
                    .collect(),
            )),
            Err(BuildGraphError::OutputNotFound("bar".into()))
        );
    }

    #[test]
    fn validate_with_dynamic_config_without_input_for_unknown_output() {
        let mut graph = BuildGraph::new(
            &[(
                "foo".into(),
                explicit_build(vec!["foo".into()], vec![]).into(),
            )]
            .into_iter()
            .collect(),
        );

        graph.validate().unwrap();

        assert_eq!(
            graph.validate_dynamic(&DynamicConfig::new(
                [("bar".into(), DynamicBuild::new(vec![]))]
                    .into_iter()
                    .collect(),
            )),
            Err(BuildGraphError::OutputNotFound("bar".into()))
        );
    }

    #[test]
    fn validate_with_header_dependencies() {
        let mut graph = BuildGraph::new(
            &[
                (
                    "foo".into(),
                    explicit_build(vec!["foo".into()], vec!["bar".into()]).into(),
                ),
                (
                    "bar".into(),
                    explicit_build(vec!["bar".into()], vec![]).into(),
                ),
            ]
            .into_iter()
            .collect(),
        );

        graph.validate().unwrap();

        graph.add_header_dependencies(&"bar".into(), &["foo".into()]);

        assert_eq!(
            graph.validate(),
            Err(BuildGraphError::CircularDependency(vec![
                "foo".into(),
                "bar".into(),
            ]))
        );
    }

    #[test]
    fn validate_without_header_dependencies() {
        let mut graph = BuildGraph::new(
            &[(
                "foo".into(),
                explicit_build(vec!["foo".into()], vec![]).into(),
            )]
            .into_iter()
            .collect(),
        );

        graph.validate().unwrap();

        graph.add_header_dependencies(&"foo".into(), &[]);

        assert_eq!(graph.validate(), Ok(()));
    }

    #[test]
    fn validate_circular_build_with_dependency_from_secondary_to_primary() {
        let build = Arc::new(explicit_build(vec!["foo".into(), "bar".into()], vec![]));

        let mut graph = BuildGraph::new(
            &[("foo".into(), build.clone()), ("bar".into(), build)]
                .into_iter()
                .collect(),
        );

        graph.validate().unwrap();

        assert_eq!(
            graph.validate_dynamic(&DynamicConfig::new(
                [("bar".into(), DynamicBuild::new(vec!["foo".into()]))]
                    .into_iter()
                    .collect(),
            )),
            Err(BuildGraphError::CircularDependency(vec!["foo".into()]))
        );
    }

    #[test]
    fn validate_circular_build_with_dependency_from_primary_to_secondary() {
        let build = Arc::new(explicit_build(vec!["foo".into(), "bar".into()], vec![]));

        let mut graph = BuildGraph::new(
            &[("foo".into(), build.clone()), ("bar".into(), build)]
                .into_iter()
                .collect(),
        );

        graph.validate().unwrap();

        assert_eq!(
            graph.validate_dynamic(&DynamicConfig::new(
                [("foo".into(), DynamicBuild::new(vec!["bar".into()]))]
                    .into_iter()
                    .collect(),
            )),
            Err(BuildGraphError::CircularDependency(vec![
                "bar".into(),
                "foo".into()
            ]))
        );
    }

    #[test]
    fn validate_circular_build_with_dependency_from_implicit_to_primary() {
        let build = Arc::new(Build::new(
            vec!["foo".into()],
            vec!["bar".into()],
            Rule::new("", None).into(),
            vec![],
            vec![],
            None,
        ));

        let mut graph = BuildGraph::new(
            &[("foo".into(), build.clone()), ("bar".into(), build)]
                .into_iter()
                .collect(),
        );

        graph.validate().unwrap();

        assert_eq!(
            graph.validate_dynamic(&DynamicConfig::new(
                [("bar".into(), DynamicBuild::new(vec!["foo".into()]))]
                    .into_iter()
                    .collect(),
            )),
            Err(BuildGraphError::CircularDependency(vec!["foo".into()]))
        );
    }

    #[test]
    fn validate_circular_build_with_dependency_from_primary_to_implicit() {
        let build = Arc::new(Build::new(
            vec!["foo".into()],
            vec!["bar".into()],
            Rule::new("", None).into(),
            vec![],
            vec![],
            None,
        ));

        let mut graph = BuildGraph::new(
            &[("foo".into(), build.clone()), ("bar".into(), build)]
                .into_iter()
                .collect(),
        );

        graph.validate().unwrap();

        assert_eq!(
            graph.validate_dynamic(&DynamicConfig::new(
                [("foo".into(), DynamicBuild::new(vec!["bar".into()]))]
                    .into_iter()
                    .collect(),
            )),
            Err(BuildGraphError::CircularDependency(vec![
                "bar".into(),
                "foo".into()
            ]))
        );
    }
}
