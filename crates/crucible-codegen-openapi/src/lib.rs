//! OpenAPI 3.1 export of the lowered Crucible JSON wire contract.
//! Smithy remains authoritative; this document is a downstream projection.

mod examples;
mod operations;
mod schemas;

pub(crate) use crucible_codegen_common::json_schema::{Slot, annotate};

use std::collections::{BTreeMap, BTreeSet};

use crucible_smithy::ir::{AuthScheme, Binding, Segment, Service, Shape, Traits};
use crucible_smithy::{Capabilities, Diagnostics, Feature, ShapeId};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// A deployment URL, including any base path. Omitted for relative URLs.
    pub endpoint: Option<String>,
}

/// Capabilities whose wire representations this emitter can describe.
pub fn capabilities() -> Capabilities {
    Capabilities::new([
        Feature::JsonName,
        Feature::Sparse,
        Feature::Sets,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::NonBlobPayload,
        Feature::ErrorPayload,
        Feature::MediaTypeHeaders,
        Feature::BasicAuth,
        Feature::ApiKeyAuth,
        Feature::DigestAuth,
        Feature::OptionalAuth,
    ])
}

/// Generate deterministic, pretty-printed OpenAPI JSON with a trailing newline.
/// Unsupported semantics and unrepresentable routes are collected as diagnostics.
pub fn generate(service: &Service, options: &Options) -> Result<String, Diagnostics> {
    let mut diagnostics = service
        .require_capabilities(&capabilities())
        .err()
        .unwrap_or_default();
    // OpenAPI has one operation per path/method, regardless of literal queries,
    // and forbids differently named templates at the same path hierarchy.
    let mut routes = BTreeSet::new();
    let mut templates = BTreeMap::new();
    let mut names = BTreeSet::new();
    for op in &service.operations {
        let path = path(&op.uri.segments);
        let canonical = op
            .uri
            .segments
            .iter()
            .map(|segment| match segment {
                Segment::Literal(value) => format!("/{value}"),
                Segment::Label(_) => "/{}".to_owned(),
            })
            .collect::<String>();
        if let Some(previous) = templates.insert(canonical, path.clone()) {
            if previous != path {
                diagnostics.push(
                    &op.id,
                    format!("OpenAPI cannot distinguish path templates {previous} and {path}"),
                );
            }
        }
        let method = op.method.to_ascii_lowercase();
        if !matches!(
            method.as_str(),
            "get" | "put" | "post" | "delete" | "options" | "head" | "patch" | "trace"
        ) {
            diagnostics.push(
                &op.id,
                format!(
                    "HTTP method {} is not representable in OpenAPI 3.1",
                    op.method
                ),
            );
        }
        if !routes.insert((path.clone(), method)) {
            diagnostics.push(&op.id, format!("OpenAPI cannot represent multiple operations on {} {path}, including routes distinguished by literal queries", op.method));
        }
        if !names.insert(service.name_of(&op.id)) {
            diagnostics.push(
                &op.id,
                "duplicate OpenAPI operationId after service renames",
            );
        }
        let mut parameters = BTreeSet::new();
        for (name, _) in &op.uri.query {
            if !parameters.insert(name.as_str()) {
                diagnostics.push(
                    &op.id,
                    format!("duplicate OpenAPI literal query parameter {name}"),
                );
            }
        }
        for binding in &op.request.query {
            if !parameters.insert(binding.name.as_str()) {
                diagnostics.push(
                    &op.id,
                    format!("duplicate OpenAPI query parameter {}", binding.name),
                );
            }
        }
        validate_headers(&mut diagnostics, &op.id, &op.request.headers, true);
        validate_headers(&mut diagnostics, &op.id, &op.response.headers, false);
    }
    for (id, shape) in &service.shapes {
        if let Shape::Structure(structure) = shape {
            if let Some(error) = &structure.error {
                validate_headers(&mut diagnostics, id, &error.headers, false);
            }
        }
    }
    for (id, scheme) in &service.auth_schemes {
        if matches!(scheme, AuthScheme::SigV4 { .. } | AuthScheme::SigV4a { .. }) {
            diagnostics.push(id, "AWS request signing is not supported by OpenAPI export");
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let mut info = json!({"title": service.title.as_deref().unwrap_or(service.id.name()), "version": service.version});
    description(&mut info, service.docs.as_deref());
    let mut schemas = Map::new();
    for (id, shape) in &service.shapes {
        schemas.insert(key(id), schemas::shape(service, shape));
    }
    let mut paths = Map::new();
    let mut ops: Vec<_> = service.operations.iter().collect();
    ops.sort_by_key(|op| &op.id);
    for op in ops {
        let item = paths
            .entry(path(&op.uri.segments))
            .or_insert_with(|| json!({}));
        item[op.method.to_ascii_lowercase()] = operations::operation(service, op);
    }
    let mut security = Map::new();
    for (id, scheme) in &service.auth_schemes {
        let value = match scheme {
            AuthScheme::Bearer => json!({"type": "http", "scheme": "bearer"}),
            AuthScheme::Basic => json!({"type": "http", "scheme": "basic"}),
            AuthScheme::Digest => json!({"type": "http", "scheme": "digest"}),
            AuthScheme::ApiKey {
                name,
                location,
                scheme,
            } => {
                let mut value = json!({"type": "apiKey", "name": name, "in": location});
                if let Some(scheme) = scheme {
                    value["description"] =
                        json!(format!("Prefix the key with {scheme} followed by a space."));
                    value["x-smithy-api-key-scheme"] = json!(scheme);
                }
                value
            }
            AuthScheme::SigV4 { .. } | AuthScheme::SigV4a { .. } => unreachable!("diagnosed above"),
        };
        security.insert(key(id), value);
    }
    let mut document = json!({
        "openapi": "3.1.1",
        "info": info,
        "paths": paths,
        "components": {"schemas": schemas, "securitySchemes": security},
        "x-smithy-service": service.id.to_string(),
        "x-smithy-protocol": service.protocol.trait_id().to_string(),
        "x-smithy-traits": traits(&service.traits),
    });
    annotate(&mut document, &service.notes, Slot::Root);
    // The permission registry with tiers and aliases; internal entries only
    // in a server-audience document, whose IR keeps them.
    let registry = crucible_codegen_common::permissions::registry(service, true);
    if registry
        .as_array()
        .is_some_and(|entries| !entries.is_empty())
    {
        document["x-crucible-permissions"] = registry;
    }
    if let Some(endpoint) = &options.endpoint {
        document["servers"] = json!([{"url": endpoint}]);
    }
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&document).expect("IR values are JSON")
    ))
}

// Fully qualified keys avoid shape, rename, and prelude collisions without
// making output depend on which other operations reach a shape.
pub(crate) fn key(id: &ShapeId) -> String {
    format!("{}.{}", id.namespace(), id.name())
}

pub(crate) fn traits(traits: &Traits) -> Value {
    Value::Object(
        traits
            .iter()
            .map(|(id, value)| (id.to_string(), value.clone()))
            .collect(),
    )
}

pub(crate) fn description(value: &mut Value, docs: Option<&str>) {
    if let Some(docs) = docs {
        value["description"] = json!(docs);
    }
}

fn path(segments: &[Segment]) -> String {
    if segments.is_empty() {
        return "/".into();
    }
    segments
        .iter()
        .map(|segment| match segment {
            Segment::Literal(value) => format!("/{value}"),
            Segment::Label(name) => format!("/{{{name}}}"),
        })
        .collect()
}

fn validate_headers(
    diagnostics: &mut Diagnostics,
    id: &ShapeId,
    bindings: &[Binding],
    request: bool,
) {
    let mut names = BTreeSet::new();
    for binding in bindings {
        let name = binding.name.to_ascii_lowercase();
        if name == "content-type"
            || (request && matches!(name.as_str(), "accept" | "authorization"))
        {
            diagnostics.push(id, format!("OpenAPI ignores a modeled {name} {} header; use media types or an authentication scheme", if request { "request" } else { "response" }));
        }
        if !names.insert(name) {
            diagnostics.push(
                id,
                format!(
                    "duplicate OpenAPI {} header {}",
                    if request { "request" } else { "response" },
                    binding.name
                ),
            );
        }
    }
}
