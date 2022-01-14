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
    pub fn new(dependencies: &HashMap<String, Arc<Build>>) -> Self {
        let mut graph = Graph::<String, ()>::new();
        let mut indexes = HashMap::<String, NodeIndex<DefaultIx>>::new();

        for output in dependencies.iter().flat_map(|(output, build)| {
            [output]
                .into_iter()
                .chain(build.inputs().iter().chain(build.order_only_inputs()))
        }) {
            indexes.insert(output.clone(), graph.add_node(output.clone()));
        }

        for (output, build) in dependencies {
            for input in build.inputs().iter().chain(build.order_only_inputs()) {
                graph.add_edge(indexes[output.as_str()], indexes[input.as_str()], ());
            }
        }

        Self { graph, indexes }
    }

    pub fn insert(&mut self, configuration: &DynamicConfiguration) {
        todo!()
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if is_cyclic_directed(&self.graph) {
            return Err(ValidationError::CircularBuildDependency);
        }

        Ok(())
    }
}
