use std::fmt;

use crate::shape_id::ShapeId;

/// A construct crucible cannot generate from, against the shape that holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub shape: ShapeId,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.shape, self.message)
    }
}

/// Every problem in a model, collected rather than stopping at the first, so
/// one run lists all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics(Vec<Diagnostic>);

impl Diagnostics {
    pub(crate) fn push(&mut self, shape: &ShapeId, message: impl Into<String>) {
        self.0.push(Diagnostic {
            shape: shape.clone(),
            message: message.into(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.0.iter()
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for diagnostic in &self.0 {
            writeln!(f, "{diagnostic}")?;
        }
        Ok(())
    }
}
