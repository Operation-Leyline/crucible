//! GetGraph.

use crate::leyline::{GetGraphInput, GetGraphOperation, Graph, GraphNotFound};
use crate::operations::Leyline;

impl GetGraphOperation for Leyline {
    async fn get_graph(&self, input: GetGraphInput) -> crucible_server_runtime_rust::Result<Graph> {
        let graphs = self.graphs.lock().unwrap();
        match graphs.get(&input.graph_id) {
            Some(name) => Ok(Graph {
                graph_id: input.graph_id,
                name: name.clone(),
            }),
            None => Err(GraphNotFound {
                message: "no such graph".to_owned(),
            }
            .into()),
        }
    }
}
