//! Crucible's front end: reads a Smithy JSON AST model and lowers one of its
//! services to the IR every emitter shares.
//!
//! The model must come from a projection with the `flattenAndRemoveMixins`
//! transform. Smithy's own flattening is the reference semantics, and a second
//! copy here would drift from it.

mod ast;
mod diagnostics;
pub mod ir;
mod lower;
mod shape_id;

use std::collections::BTreeMap;

pub use diagnostics::{Diagnostic, Diagnostics};
pub use shape_id::{ShapeId, ShapeIdError};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("model is not a Smithy JSON AST: {0}")]
    Json(#[from] serde_json::Error),
    #[error("model is Smithy IDL {0}; crucible reads 2.0")]
    Version(String),
    #[error("model defines no service")]
    NoService,
    #[error("model defines several services; choose one of {}", list(.0))]
    AmbiguousService(Vec<ShapeId>),
    #[error("model has no service {0}")]
    UnknownService(ShapeId),
    #[error("{} unsupported or malformed construct(s):\n{}", .0.len(), .0)]
    Unsupported(Diagnostics),
}

fn list(ids: &[ShapeId]) -> String {
    ids.iter()
        .map(ShapeId::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// A parsed model: every shape, the prelude added, `apply` blocks merged.
#[derive(Debug)]
pub struct Model {
    shapes: BTreeMap<ShapeId, ast::Shape>,
}

impl Model {
    pub fn from_json(json: &str) -> Result<Self, Error> {
        let model: ast::Model = serde_json::from_str(json)?;
        if !model.smithy.starts_with("2.") {
            return Err(Error::Version(model.smithy));
        }
        let mut shapes = model.shapes;
        for (id, shape) in ast::prelude() {
            shapes.entry(id).or_insert(shape);
        }
        merge_applies(&mut shapes);
        Ok(Self { shapes })
    }

    pub fn services(&self) -> Vec<ShapeId> {
        self.shapes
            .iter()
            .filter(|(_, shape)| shape.kind == ast::Kind::Service)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Lower `service`, or the model's only service when `None`.
    pub fn lower(&self, service: Option<&ShapeId>) -> Result<ir::Service, Error> {
        let id = match service {
            Some(id) if self.services().contains(id) => id.clone(),
            Some(id) => return Err(Error::UnknownService(id.clone())),
            None => match self.services().as_slice() {
                [] => return Err(Error::NoService),
                [only] => only.clone(),
                many => return Err(Error::AmbiguousService(many.to_vec())),
            },
        };
        lower::lower(&self.shapes, &id).map_err(Error::Unsupported)
    }
}

/// Smithy's built JSON AST normally has none left, but a hand-written one may.
/// An `apply` whose target is missing is left alone: lowering reports a
/// missing target only where something uses it.
fn merge_applies(shapes: &mut BTreeMap<ShapeId, ast::Shape>) {
    let applies: Vec<(ShapeId, ast::Traits)> = shapes
        .iter()
        .filter(|(_, shape)| shape.kind == ast::Kind::Apply)
        .map(|(id, shape)| (id.clone(), shape.traits.clone()))
        .collect();
    for (id, traits) in applies {
        shapes.remove(&id);
        let Some(shape) = shapes.get_mut(&id.root()) else {
            continue;
        };
        let target = match id.member() {
            None => &mut shape.traits,
            Some(member) => match shape.members.get_mut(member) {
                Some(member) => &mut member.traits,
                None => continue,
            },
        };
        target.extend(traits);
    }
}
