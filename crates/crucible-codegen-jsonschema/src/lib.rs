//! Standalone Draft 2020-12 document schemas from the lowered, audience-projected IR.

use crucible_ir::{Capabilities, Diagnostics, Feature, ShapeId, ir::Service};

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// A fully qualified shape id or an unambiguous original shape name.
    pub shape: String,
}

/// Document schemas describe values independently of HTTP route/header bindings.
pub fn capabilities() -> Capabilities {
    Capabilities::new([
        Feature::GreedyLabels,
        Feature::Sparse,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::NonBlobPayload,
        Feature::ErrorPayload,
        Feature::MediaTypeHeaders,
    ])
}

/// Resolve a named shape within the selected service and audience.
/// Service renames do not change schema identities or short names.
pub fn resolve_shape(service: &Service, name: &str) -> Result<ShapeId, Diagnostics> {
    let mut diagnostics = Diagnostics::default();
    let parsed = if name.contains('#') {
        name.parse::<ShapeId>()
    } else {
        format!("selection#{name}").parse::<ShapeId>()
    };
    let valid = parsed.as_ref().is_ok_and(|id| id.member().is_none());
    if !valid {
        diagnostics.push(
            &service.id,
            format!("invalid JSON Schema shape {name:?}; expected namespace#Name or Name"),
        );
        return Err(diagnostics);
    }
    let candidates: Vec<_> = service
        .shapes
        .keys()
        .filter(|id| {
            if name.contains('#') {
                Some(*id) == parsed.as_ref().ok()
            } else {
                id.name() == name
            }
        })
        .collect();
    match candidates.as_slice() {
        [id] => Ok((*id).clone()),
        [] => {
            diagnostics.push(&service.id, format!("JSON Schema shape {name:?} is unavailable in the selected service/audience; it may be unknown, unreachable, or removed by audience projection"));
            Err(diagnostics)
        }
        _ => {
            diagnostics.push(
                &service.id,
                format!(
                    "ambiguous JSON Schema shape {name:?}; use a fully qualified id: {}",
                    candidates
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
            Err(diagnostics)
        }
    }
}

/// Emit a self-contained document with only the root's dependency closure in `$defs`.
/// Callers project audience and API version before invoking this generator.
pub fn generate(service: &Service, options: &Options) -> Result<String, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    let id = resolve_shape(service, &options.shape)?;
    let mut document = crucible_codegen_common::json_schema::standalone(service, Some(&id));
    document["$schema"] = "https://json-schema.org/draft/2020-12/schema".into();
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&document).expect("IR values are JSON")
    ))
}
