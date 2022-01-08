use crate::ast::Module;
use petgraph::Graph;

pub fn compile(_module: &Module) -> Result<Graph<String, ()>, String> {
    let graph = Graph::new();

    Ok(graph)
}
