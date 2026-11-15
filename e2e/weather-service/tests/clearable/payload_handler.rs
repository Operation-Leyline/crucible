use std::sync::{Arc, Mutex};

use crucible_server_runtime_rust::{Field, Result};

use super::payloads::notes;

pub struct PayloadHandler(pub Arc<Mutex<Vec<&'static str>>>);

impl PayloadHandler {
    fn record<T>(&self, value: Field<T>) {
        self.0.lock().unwrap().push(match value {
            Field::Absent => "absent",
            Field::Null => "null",
            Field::Value(_) => "value",
        });
    }
}

impl notes::SetDocumentOperation for PayloadHandler {
    async fn set_document(&self, input: notes::SetDocumentInput) -> Result<()> {
        self.record(input.value);
        Ok(())
    }
}
impl notes::SetStructureOperation for PayloadHandler {
    async fn set_structure(&self, input: notes::SetStructureInput) -> Result<()> {
        self.record(input.value);
        Ok(())
    }
}
impl notes::SetUnionOperation for PayloadHandler {
    async fn set_union(&self, input: notes::SetUnionInput) -> Result<()> {
        self.record(input.value);
        Ok(())
    }
}
