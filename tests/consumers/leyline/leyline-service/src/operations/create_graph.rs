//! CreateGraph.

use crate::leyline::{
    CreateGraphInput, CreateGraphOperation, CreateGraphOutput, ValidationException, ValidationExceptionField,
    ValidationExceptionReason,
};
use crate::operations::Leyline;

impl CreateGraphOperation for Leyline {
    async fn create_graph(
        &self,
        input: CreateGraphInput,
    ) -> crucible_server_runtime_rust::Result<CreateGraphOutput> {
        // `@length(min: 1)` admits "  "; a business rule the model cannot state
        // answers with the same typed reason the framework uses for the model's.
        if input.name.trim().is_empty() {
            return Err(ValidationException {
                message: "the graph needs a name".to_owned(),
                reason: ValidationExceptionReason::FieldValidationFailed,
                field_list: Some(vec![ValidationExceptionField {
                    name: "name".to_owned(),
                    message: "must not be blank".to_owned(),
                }]),
            }
            .into());
        }
        let mut graphs = self.graphs.lock().unwrap();
        let graph_id = format!("g-{}", graphs.len() + 1);
        graphs.insert(graph_id.clone(), input.name.clone());
        Ok(CreateGraphOutput {
            graph_id,
            name: input.name,
        })
    }
}
