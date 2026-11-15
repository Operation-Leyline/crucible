use std::fmt::Display;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crucible_server_runtime_rust::{Field, Result};

use super::notes;

pub struct NotesHandler(pub Arc<AtomicUsize>);

fn state<T: Display>(value: Field<T>) -> String {
    match value {
        Field::Absent => "absent".to_owned(),
        Field::Null => "null".to_owned(),
        Field::Value(value) => format!("value:{value}"),
    }
}

impl notes::PatchOperation for NotesHandler {
    async fn patch(&self, input: notes::PatchInput) -> Result<notes::PatchOutput> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(notes::PatchOutput {
            description_state: state(input.description),
            rating_state: state(input.rating),
            ordinary_present: input.ordinary.is_some(),
        })
    }
}
