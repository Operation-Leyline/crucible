//! Crucible's front end: reads a Smithy JSON AST model and lowers one of its
//! services to the IR every emitter shares.
//!
//! The model must come from a projection with the `flattenAndRemoveMixins`
//! transform. Smithy's own flattening is the reference semantics, and a second
//! copy here would drift from it.

mod ast;
mod capabilities;
mod diagnostics;
mod discovery;
mod extensions;
pub mod ir;
mod jmespath_ir;
mod lower;
mod lower_checksums;
mod lower_endpoints;
mod lower_extensions;
mod lower_streams;
mod names;
mod pattern;
mod protocol;
mod protocol_tests;
mod recursion;
mod resources;
mod shape_id;
mod timestamp_default;
mod xml;

use std::collections::BTreeMap;

pub use capabilities::{Capabilities, Feature, FeatureUse};
pub use diagnostics::{Diagnostic, Diagnostics};
pub use protocol::{Protocol, ProtocolSettings};
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

    /// Lower the operation contract for SDKs and servers. Resource-only shapes
    /// and provisioning diagnostics are excluded. `None` selects the only service.
    pub fn lower(&self, service: Option<&ShapeId>) -> Result<ir::Service, Error> {
        self.lower_with_capabilities(service, &Capabilities::default())
    }

    /// Lower only semantics that the intended emitter explicitly supports.
    pub fn lower_with_capabilities(
        &self,
        service: Option<&ShapeId>,
        capabilities: &Capabilities,
    ) -> Result<ir::Service, Error> {
        self.lower_selected(service, capabilities, None, false)
    }

    /// Explicit protocol selection for services that declare alternatives.
    pub fn lower_with_protocol(
        &self,
        service: Option<&ShapeId>,
        capabilities: &Capabilities,
        protocol: Protocol,
    ) -> Result<ir::Service, Error> {
        self.lower_selected(service, capabilities, Some(protocol), false)
    }

    /// Lower the operation contract plus resource metadata and resource-only
    /// shapes for infrastructure generators. Includes provisioning diagnostics.
    pub fn lower_infrastructure(&self, service: Option<&ShapeId>) -> Result<ir::Service, Error> {
        self.lower_selected(service, &Capabilities::default(), None, true)
    }

    /// Lower resource metadata with an explicitly selected declared protocol.
    pub fn lower_infrastructure_with_protocol(
        &self,
        service: Option<&ShapeId>,
        protocol: Protocol,
    ) -> Result<ir::Service, Error> {
        self.lower_selected(service, &Capabilities::default(), Some(protocol), true)
    }

    fn lower_selected(
        &self,
        service: Option<&ShapeId>,
        capabilities: &Capabilities,
        protocol: Option<Protocol>,
        infrastructure: bool,
    ) -> Result<ir::Service, Error> {
        let id = match service {
            Some(id) if self.services().contains(id) => id.clone(),
            Some(id) => return Err(Error::UnknownService(id.clone())),
            None => match self.services().as_slice() {
                [] => return Err(Error::NoService),
                [only] => only.clone(),
                many => return Err(Error::AmbiguousService(many.to_vec())),
            },
        };
        lower::lower(&self.shapes, &id, capabilities, protocol, infrastructure)
            .map_err(Error::Unsupported)
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
