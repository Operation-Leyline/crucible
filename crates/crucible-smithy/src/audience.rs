//! Audience semantics operate only on lowered IR, independently of an emitter.
use std::collections::{BTreeMap, BTreeSet};

use crate::ir::{
    Bindings, EndpointParameterSource, HostPrefixPart, JmesExpression, Member, Operation, Service,
    Shape, WaiterMatcher,
};
use crate::{Diagnostics, ShapeId};

/// Public packages omit internal API unless their producer explicitly opts in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Audience {
    #[default]
    External,
    Internal,
}

/// Project a contract without mutating the full model used by servers and policy.
/// Refusals are collected before returning; callers must not publish partial output.
pub fn project(service: &Service, audience: Audience) -> Result<Service, Diagnostics> {
    if audience == Audience::Internal {
        return Ok(service.clone());
    }
    let mut result = service.clone();
    result
        .errors
        .retain(|id| !service.internal && !service.shape(id).is_some_and(Shape::internal));
    // CORS describes the deployed server contract, which audience selection
    // never projects. Keep its full header and method lists on client views too.
    result
        .operations
        .retain(|op| !service.internal && !op.internal);
    for op in &mut result.operations {
        op.errors
            .retain(|id| !service.shape(id).is_some_and(Shape::internal));
    }
    let operations: BTreeSet<_> = result.operations.iter().map(|op| op.id.clone()).collect();
    result
        .resources
        .retain(|_, resource| !service.internal && !resource.internal);
    // Only resources bound through surviving roots are part of this audience.
    let mut resources = BTreeSet::new();
    let mut pending = service.resource_roots.clone();
    while let Some(id) = pending.pop() {
        if let Some(resource) = result.resources.get(&id)
            && resources.insert(id)
        {
            pending.extend(resource.resources.iter().cloned());
        }
    }
    result.resources.retain(|id, _| resources.contains(id));
    result.resource_roots.retain(|id| resources.contains(id));
    let mut diagnostics = Diagnostics::default();
    for resource in result.resources.values_mut() {
        resource.resources.retain(|id| resources.contains(id));
        for (name, slot) in [
            ("create", &mut resource.lifecycle.create),
            ("put", &mut resource.lifecycle.put),
            ("read", &mut resource.lifecycle.read),
            ("update", &mut resource.lifecycle.update),
            ("delete", &mut resource.lifecycle.delete),
            ("list", &mut resource.lifecycle.list),
        ] {
            if let Some(id) = slot.as_ref()
                && !operations.contains(id)
            {
                diagnostics.push(&resource.id, format!("audience.resource_lifecycle: external resource {name} lifecycle requires removed internal operation {id}"));
                *slot = None;
            }
        }
        resource.operations.retain(|id| operations.contains(id));
        resource
            .collection_operations
            .retain(|id| operations.contains(id));
        resource.bindings.retain(|id, _| operations.contains(id));
    }
    let mut reached = BTreeSet::new();
    let mut pending: Vec<_> = result
        .operations
        .iter()
        .flat_map(|op| op.input.iter().chain(&op.output).chain(&op.errors))
        .chain(&result.errors)
        .cloned()
        .collect();
    for resource in result.resources.values() {
        for target in resource
            .identifiers
            .values()
            .chain(resource.properties.values())
        {
            check_target(service, &resource.id, target, &mut diagnostics);
            pending.push(target.clone());
        }
    }
    for op in &result.operations {
        for target in op.input.iter().chain(&op.output) {
            check_target(service, &op.id, target, &mut diagnostics);
        }
        check_references(service, op, &operations, &mut diagnostics);
    }
    if let Some(discovery) = &service.endpoint_discovery
        && let Some(op) = result
            .operations
            .iter()
            .find(|op| op.id == discovery.operation)
    {
        for path in discovery
            .operation_member
            .iter()
            .chain(&discovery.identifiers_member)
        {
            check_path(
                service,
                op,
                op.input.as_ref(),
                path,
                "endpoint",
                &mut diagnostics,
            );
        }
        for path in [
            discovery.endpoints_member.clone(),
            format!(
                "{}.{}",
                discovery.endpoints_member, discovery.address_member
            ),
            format!(
                "{}.{}",
                discovery.endpoints_member, discovery.cache_period_member
            ),
        ] {
            check_path(
                service,
                op,
                op.output.as_ref(),
                &path,
                "endpoint",
                &mut diagnostics,
            );
        }
        if let Some(error) = &discovery.error
            && service.shape(error).is_some_and(Shape::internal)
        {
            diagnostics.push(&service.id, format!("audience.endpoint_reference: endpoint discovery requires removed internal error {error}"));
        }
    }
    while let Some(id) = pending.pop() {
        if !reached.insert(id.clone()) {
            continue;
        }
        let Some(shape) = service.shape(&id) else {
            continue;
        };
        if shape.internal() {
            continue;
        }
        for member in shape.members() {
            let member_id = id.with_member(&member.name);
            if member.internal {
                if member.required {
                    diagnostics.push(&member_id, "audience.required_member: external contract requires an internal member that cannot be omitted");
                }
                if matches!(shape, Shape::List(_) | Shape::Map(_)) {
                    diagnostics.push(&member_id, "audience.collection_member: external collection cannot omit its internal member");
                }
            } else {
                check_target(service, &member_id, &member.target, &mut diagnostics);
                if removed_enum_default(service, member) {
                    diagnostics.push(&member_id, "audience.default_reference: external default names a removed internal enum value");
                }
                pending.push(member.target.clone());
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    result
        .shapes
        .retain(|id, shape| reached.contains(id) && !shape.internal());
    for shape in result.shapes.values_mut() {
        match shape {
            Shape::Structure(s) | Shape::Union(s) => {
                s.members.retain(|m| !m.internal);
                if let Some(error) = &mut s.error {
                    let names: BTreeSet<_> = s.members.iter().map(|m| m.name.as_str()).collect();
                    error.headers.retain(|b| names.contains(b.member.as_str()));
                    error
                        .prefix_headers
                        .retain(|b| names.contains(b.member.as_str()));
                    error
                        .timestamp_formats
                        .retain(|n, _| names.contains(n.as_str()));
                    error
                        .header_encodings
                        .retain(|n, _| names.contains(n.as_str()));
                    error.response_code = error
                        .response_code
                        .take()
                        .filter(|n| names.contains(n.as_str()));
                    error.payload = error
                        .payload
                        .take()
                        .filter(|p| names.contains(p.member.as_str()));
                }
            }
            Shape::Enum(s) => s.values.retain(|v| !v.internal),
            Shape::IntEnum(s) => s.values.retain(|v| !v.internal),
            _ => {}
        }
    }
    project_permissions(&mut result);
    for op in &mut result.operations {
        retain_bindings(&result.shapes, op.input.as_ref(), &mut op.request);
        retain_bindings(&result.shapes, op.output.as_ref(), &mut op.response);
        op.idempotency_token = op
            .idempotency_token
            .take()
            .filter(|n| member_exists(&result.shapes, op.input.as_ref(), n));
        op.resource = op
            .resource
            .take()
            .filter(|id| resources.contains(id) || service.resources.is_empty());
        // Examples are model values too: project nested structure/union members,
        // not only their top-level bindings, before any emitter can see them.
        for example in &mut op.examples {
            let mut valid = true;
            if let Some(value) = &mut example.input {
                valid &= project_example(&result.shapes, op.input.as_ref(), value);
            }
            if let Some(value) = &mut example.output {
                valid &= project_example(&result.shapes, op.output.as_ref(), value);
            }
            example.error = example
                .error
                .take()
                .filter(|e| op.errors.contains(&e.shape));
            if let Some(error) = &mut example.error {
                valid &= project_example(&result.shapes, Some(&error.shape), &mut error.content);
            }
            if !valid {
                diagnostics.push(&op.id, format!("audience.example_reference: example {:?} uses a removed internal union arm or enum value", example.title));
            }
        }
    }
    result
        .resource_references
        .retain(|id, _| reached.contains(id));
    for references in result.resource_references.values_mut() {
        references.retain(|r| !r.resolved || resources.contains(&r.resource));
    }
    result.xml_shapes.retain(|id, _| reached.contains(id));
    for (id, xml) in &mut result.xml_shapes {
        xml.members
            .retain(|name, _| member_exists(&result.shapes, Some(id), name));
    }
    result.recursive_groups.retain_mut(|group| {
        group.shapes.retain(|id| reached.contains(id));
        group.members.retain(|id| {
            id.member()
                .is_some_and(|name| member_exists(&result.shapes, Some(&id.root()), name))
        });
        !group.shapes.is_empty()
    });
    result.requirements.retain(|usage| {
        let root = usage.shape.root();
        if root == service.id {
            return true;
        }
        if operations.contains(&root) || resources.contains(&root) {
            return true;
        }
        reached.contains(&root)
            && usage
                .shape
                .member()
                .is_none_or(|name| member_exists(&result.shapes, Some(&root), name))
    });
    crate::audience_protocol_tests::retain(service, &mut result);
    result.endpoint_discovery = result
        .endpoint_discovery
        .take()
        .filter(|d| operations.contains(&d.operation));
    result
        .renames
        .retain(|id, _| reached.contains(id) || operations.contains(id) || resources.contains(id));
    strip_raw_traits(&mut result);
    if diagnostics.is_empty() {
        Ok(result)
    } else {
        Err(diagnostics)
    }
}

/// Standard traits (the `smithy.*`, `aws.*` and `crucible.*` namespaces)
/// an external view keeps in its raw trait maps: their values are scalars,
/// wire bindings, constraints or protocol settings, never a member, shape or
/// enum value the projection may have removed. The rest (`smithy.api#enum`,
/// `smithy.api#references`, protocol tests, examples, waiters, endpoint
/// tests, and any standard trait not listed) are dropped; their projected
/// typed forms remain.
const EXTERNAL_RAW_TRAITS: &[&str] = &[
    "smithy.api#addedDefault",
    "smithy.api#auth",
    "smithy.api#authDefinition",
    "smithy.api#box",
    "smithy.api#clientOptional",
    "smithy.api#cors",
    "smithy.api#default",
    "smithy.api#deprecated",
    "smithy.api#documentation",
    "smithy.api#endpoint",
    "smithy.api#enumValue",
    "smithy.api#error",
    "smithy.api#eventHeader",
    "smithy.api#eventPayload",
    "smithy.api#externalDocumentation",
    "smithy.api#hostLabel",
    "smithy.api#http",
    "smithy.api#httpApiKeyAuth",
    "smithy.api#httpBasicAuth",
    "smithy.api#httpBearerAuth",
    "smithy.api#httpChecksumRequired",
    "smithy.api#httpDigestAuth",
    "smithy.api#httpError",
    "smithy.api#httpHeader",
    "smithy.api#httpLabel",
    "smithy.api#httpPayload",
    "smithy.api#httpPrefixHeaders",
    "smithy.api#httpQuery",
    "smithy.api#httpQueryParams",
    "smithy.api#httpResponseCode",
    "smithy.api#idempotencyToken",
    "smithy.api#idempotent",
    "smithy.api#input",
    "smithy.api#jsonName",
    "smithy.api#length",
    "smithy.api#mediaType",
    "smithy.api#noReplace",
    "smithy.api#optionalAuth",
    "smithy.api#output",
    "smithy.api#paginated",
    "smithy.api#pattern",
    "smithy.api#private",
    "smithy.api#protocolDefinition",
    "smithy.api#range",
    "smithy.api#readonly",
    "smithy.api#recommended",
    "smithy.api#requestCompression",
    "smithy.api#required",
    "smithy.api#requiresLength",
    "smithy.api#retryable",
    "smithy.api#sensitive",
    "smithy.api#since",
    "smithy.api#sparse",
    "smithy.api#streaming",
    "smithy.api#tags",
    "smithy.api#timestampFormat",
    "smithy.api#title",
    "smithy.api#uniqueItems",
    "smithy.api#unitType",
    "smithy.api#unstable",
    "smithy.api#xmlAttribute",
    "smithy.api#xmlFlattened",
    "smithy.api#xmlName",
    "smithy.api#xmlNamespace",
    "smithy.protocols#rpcv2Cbor",
    "smithy.protocols#rpcv2Json",
    "smithy.rules#clientContextParams",
    "smithy.rules#contextParam",
    "smithy.rules#endpointRuleSet",
    "smithy.rules#staticContextParams",
    "aws.api#service",
    "aws.auth#sigv4",
    "aws.auth#unsignedPayload",
    "aws.protocols#awsJson1_0",
    "aws.protocols#awsJson1_1",
    "aws.protocols#awsQueryError",
    "aws.protocols#httpChecksum",
    "aws.protocols#restJson1",
    "aws.protocols#restXml",
    "crucible.auth#scopes",
    "crucible.protocols#json",
    "crucible.protocols#strict",
];

/// Whether an external view keeps a raw trait application. A custom trait
/// (outside the standard namespaces) is the author's: it stays unless its
/// definition is `@internal`, which is Smithy's own projection rule.
fn external_raw_trait(id: &ShapeId, internal_traits: &BTreeSet<ShapeId>) -> bool {
    let namespace = id.namespace();
    let standard = namespace.starts_with("smithy.")
        || namespace.starts_with("aws.")
        || namespace.starts_with("crucible.");
    if standard {
        EXTERNAL_RAW_TRAITS.contains(&id.to_string().as_str())
    } else {
        !internal_traits.contains(id)
    }
}

/// Every IR node keeps a raw copy of its Smithy traits. The projection
/// rewrites the typed fields, not those copies, so a removed vector, enum
/// value or resource could survive in them: an external view keeps only
/// the raw traits [`external_raw_trait`] allows, and is read through the
/// typed fields for the rest.
fn strip_raw_traits(service: &mut Service) {
    let internal_traits = std::mem::take(&mut service.internal_traits);
    let strip = |traits: &mut crate::ir::Traits| {
        traits.retain(|id, _| external_raw_trait(id, &internal_traits));
    };
    strip(&mut service.traits);
    for op in &mut service.operations {
        strip(&mut op.traits);
    }
    for resource in service.resources.values_mut() {
        strip(&mut resource.traits);
    }
    for shape in service.shapes.values_mut() {
        match shape {
            Shape::Unit { .. } => {}
            Shape::Simple(s) => strip(&mut s.traits),
            Shape::Enum(s) => {
                strip(&mut s.traits);
                s.values.iter_mut().for_each(|v| strip(&mut v.traits));
            }
            Shape::IntEnum(s) => {
                strip(&mut s.traits);
                s.values.iter_mut().for_each(|v| strip(&mut v.traits));
            }
            Shape::List(s) => {
                strip(&mut s.traits);
                strip(&mut s.member.traits);
            }
            Shape::Map(s) => {
                strip(&mut s.traits);
                strip(&mut s.key.traits);
                strip(&mut s.value.traits);
            }
            Shape::Structure(s) | Shape::Union(s) => {
                strip(&mut s.traits);
                s.members.iter_mut().for_each(|m| strip(&mut m.traits));
            }
        }
    }
}

fn project_example(
    shapes: &BTreeMap<ShapeId, Shape>,
    id: Option<&ShapeId>,
    value: &mut serde_json::Value,
) -> bool {
    match (id.and_then(|id| shapes.get(id)), value) {
        (
            Some(shape @ (Shape::Structure(s) | Shape::Union(s))),
            serde_json::Value::Object(values),
        ) => {
            values.retain(|name, _| s.member(name).is_some());
            let mut valid = !matches!(shape, Shape::Union(_)) || !values.is_empty();
            for (name, value) in values {
                valid &= project_example(shapes, s.member(name).map(|m| &m.target), value);
            }
            valid
        }
        (Some(Shape::List(s)), serde_json::Value::Array(values)) => values
            .iter_mut()
            .all(|value| project_example(shapes, Some(&s.member.target), value)),
        (Some(Shape::Map(s)), serde_json::Value::Object(values)) => values
            .values_mut()
            .all(|value| project_example(shapes, Some(&s.value.target), value)),
        (Some(Shape::Enum(s)), serde_json::Value::String(value)) => {
            s.values.iter().any(|v| v.value == *value)
        }
        (Some(Shape::IntEnum(s)), serde_json::Value::Number(value)) => s
            .values
            .iter()
            .any(|v| Some(i64::from(v.value)) == value.as_i64()),
        _ => true,
    }
}

fn removed_enum_default(service: &Service, member: &Member) -> bool {
    // Input/client-optional defaults are ignored by clients, so they do not
    // reference a generated enum variant in the projected contract.
    match (service.shape(&member.target), member.client_default()) {
        (Some(Shape::Enum(shape)), Some(crate::ir::Value::String(value))) => shape
            .values
            .iter()
            .any(|variant| variant.internal && variant.value == *value),
        (Some(Shape::IntEnum(shape)), Some(crate::ir::Value::Number(value))) => shape
            .values
            .iter()
            .any(|variant| variant.internal && Some(i64::from(variant.value)) == value.as_i64()),
        _ => false,
    }
}

fn check_target(
    service: &Service,
    owner: &ShapeId,
    target: &ShapeId,
    diagnostics: &mut Diagnostics,
) {
    if service.shape(target).is_some_and(Shape::internal) {
        diagnostics.push(
            owner,
            format!("audience.internal_target: external contract targets internal shape {target}"),
        );
    }
}

fn member_exists(shapes: &BTreeMap<ShapeId, Shape>, id: Option<&ShapeId>, name: &str) -> bool {
    id.and_then(|id| shapes.get(id))
        .is_some_and(|s| s.members().iter().any(|m| m.name == name))
}

fn retain_bindings(
    shapes: &BTreeMap<ShapeId, Shape>,
    id: Option<&ShapeId>,
    bindings: &mut Bindings,
) {
    let keep = |name: &str| member_exists(shapes, id, name);
    bindings.labels.retain(|name| keep(name));
    bindings.body.retain(|name| keep(name));
    bindings.query.retain(|b| keep(&b.member));
    bindings.headers.retain(|b| keep(&b.member));
    bindings.prefix_headers.retain(|b| keep(&b.member));
    bindings.timestamp_formats.retain(|n, _| keep(n));
    bindings.header_encodings.retain(|n, _| keep(n));
    bindings.query_params = bindings.query_params.take().filter(|n| keep(n));
    bindings.response_code = bindings.response_code.take().filter(|n| keep(n));
    bindings.payload = bindings.payload.take().filter(|p| keep(&p.member));
    bindings.event_stream = bindings.event_stream.take().filter(|p| keep(&p.member));
}

fn check_path(
    service: &Service,
    op: &Operation,
    root: Option<&ShapeId>,
    path: &str,
    kind: &str,
    diagnostics: &mut Diagnostics,
) {
    let mut current = root.cloned();
    for name in path.split('.') {
        let Some(mut id) = current else {
            break;
        };
        // Discovery paths cross a list of endpoint structures. Resolve the
        // element type while preserving collection-member diagnostics elsewhere.
        if let Some(Shape::List(list)) = service.shape(&id) {
            id = list.member.target.clone();
        }
        let Some(member) = service
            .shape(&id)
            .and_then(|s| s.members().into_iter().find(|m| m.name == name))
        else {
            break;
        };
        if member.internal {
            diagnostics.push(
                &op.id,
                format!(
                    "audience.{kind}_reference: {kind} references removed member {}",
                    id.with_member(name)
                ),
            );
        }
        current = Some(member.target.clone());
    }
}

fn check_references(
    service: &Service,
    op: &Operation,
    operations: &BTreeSet<ShapeId>,
    diagnostics: &mut Diagnostics,
) {
    if let Some(p) = &op.paginated {
        for path in std::iter::once(&p.input_token).chain(&p.page_size) {
            check_path(
                service,
                op,
                op.input.as_ref(),
                path,
                "paginator",
                diagnostics,
            );
        }
        for path in std::iter::once(&p.output_token).chain(&p.items) {
            check_path(
                service,
                op,
                op.output.as_ref(),
                path,
                "paginator",
                diagnostics,
            );
        }
    }
    if let Some(endpoint) = &op.endpoint {
        for part in &endpoint.parts {
            if let HostPrefixPart::Label { member } = part {
                check_path(
                    service,
                    op,
                    op.input.as_ref(),
                    member,
                    "endpoint",
                    diagnostics,
                );
            }
        }
    }
    for sources in op.endpoint_parameters.values() {
        for source in sources {
            match source {
                EndpointParameterSource::InputMember { member } => check_path(
                    service,
                    op,
                    op.input.as_ref(),
                    member,
                    "endpoint",
                    diagnostics,
                ),
                EndpointParameterSource::InputPath { path } => check_expression(
                    service,
                    op,
                    op.input.as_ref(),
                    &path.expression,
                    "endpoint",
                    false,
                    diagnostics,
                ),
                _ => {}
            }
        }
    }
    for waiter in op.waiters.values() {
        for acceptor in &waiter.acceptors {
            match &acceptor.matcher {
                WaiterMatcher::Output { path, .. } => check_expression(service, op, op.output.as_ref(), &path.expression, "waiter", false, diagnostics),
                WaiterMatcher::InputOutput { path, .. } => check_expression(service, op, None, &path.expression, "waiter", true, diagnostics),
                WaiterMatcher::ErrorType { name } if service.operations.iter().find(|o| o.id == op.id).is_some_and(|original| original.errors.iter().any(|id| (id.name() == name || id.to_string() == *name) && !op.errors.contains(id))) => diagnostics.push(&op.id, format!("audience.waiter_reference: waiter references removed internal error {name}")),
                _ => {}
            }
            if let Some(path) = &acceptor.message {
                check_expression(
                    service,
                    op,
                    op.output.as_ref(),
                    &path.expression,
                    "waiter",
                    false,
                    diagnostics,
                );
            }
        }
    }
    if let Some(discovery) = &service.endpoint_discovery {
        if op.discovered_endpoint.is_some() && !operations.contains(&discovery.operation) {
            diagnostics.push(
                &op.id,
                format!(
                    "audience.endpoint_reference: endpoint discovery requires removed operation {}",
                    discovery.operation
                ),
            );
        }
    }
    if let Some(crate::ir::Checksum::AwsFlexible {
        request_algorithm_member,
        validation_mode_member,
        ..
    }) = &op.checksum
    {
        for member in request_algorithm_member
            .iter()
            .chain(validation_mode_member)
        {
            check_path(
                service,
                op,
                op.input.as_ref(),
                member,
                "checksum",
                diagnostics,
            );
        }
    }
}

fn check_expression(
    service: &Service,
    op: &Operation,
    root: Option<&ShapeId>,
    expression: &JmesExpression,
    kind: &str,
    input_output: bool,
    diagnostics: &mut Diagnostics,
) {
    let mut removed = BTreeSet::new();
    expression_targets(
        service,
        op,
        root.into_iter().cloned().collect(),
        expression,
        input_output,
        &mut removed,
    );
    for id in removed {
        diagnostics.push(
            &op.id,
            format!("audience.{kind}_reference: {kind} references removed member {id}"),
        );
    }
}

// Carry model types through compiled expressions so a nested field is checked
// against its actual structure rather than a global set of member spellings.
fn expression_targets(
    service: &Service,
    op: &Operation,
    roots: Vec<ShapeId>,
    expression: &JmesExpression,
    input_output: bool,
    removed: &mut BTreeSet<ShapeId>,
) -> Vec<ShapeId> {
    use JmesExpression as E;
    let elements = |roots: Vec<ShapeId>| {
        roots
            .into_iter()
            .flat_map(|id| match service.shape(&id) {
                Some(Shape::List(s)) => vec![s.member.target.clone()],
                Some(Shape::Map(s)) => vec![s.value.target.clone()],
                _ => vec![id],
            })
            .collect::<Vec<_>>()
    };
    match expression {
        E::Field { name } => {
            if input_output {
                return match name.as_str() {
                    "input" => op.input.iter().cloned().collect(),
                    "output" => op.output.iter().cloned().collect(),
                    _ => vec![],
                };
            }
            roots
                .iter()
                .filter_map(|id| {
                    let member = service
                        .shape(id)?
                        .members()
                        .into_iter()
                        .find(|m| m.name == *name)?;
                    if member.internal {
                        removed.insert(id.with_member(name));
                    }
                    Some(member.target.clone())
                })
                .collect()
        }
        E::Subexpression { left, right } => {
            let targets = expression_targets(service, op, roots, left, input_output, removed);
            expression_targets(service, op, targets, right, false, removed)
        }
        E::Projection { left, right } => {
            let targets = elements(expression_targets(
                service,
                op,
                roots,
                left,
                input_output,
                removed,
            ));
            expression_targets(service, op, targets, right, false, removed)
        }
        E::Flatten { expression } | E::ObjectValues { expression } => elements(expression_targets(
            service,
            op,
            roots,
            expression,
            input_output,
            removed,
        )),
        E::Index { .. } | E::Slice { .. } => elements(roots),
        E::And { left, right } | E::Or { left, right } | E::Compare { left, right, .. } => {
            let mut targets =
                expression_targets(service, op, roots.clone(), left, input_output, removed);
            targets.extend(expression_targets(
                service,
                op,
                roots,
                right,
                input_output,
                removed,
            ));
            targets
        }
        E::Condition { predicate, then } => {
            expression_targets(service, op, roots.clone(), predicate, input_output, removed);
            expression_targets(service, op, roots, then, input_output, removed)
        }
        E::Not { expression } | E::ExpressionReference { expression } => {
            expression_targets(service, op, roots, expression, input_output, removed)
        }
        E::MultiList { expressions } => expressions
            .iter()
            .flat_map(|e| expression_targets(service, op, roots.clone(), e, input_output, removed))
            .collect(),
        E::MultiHash { entries } => entries
            .iter()
            .flat_map(|e| {
                expression_targets(service, op, roots.clone(), &e.value, input_output, removed)
            })
            .collect(),
        E::Function { arguments, .. } => {
            let targets: Vec<_> = arguments
                .iter()
                .filter(|e| !matches!(e, E::ExpressionReference { .. }))
                .flat_map(|e| {
                    expression_targets(service, op, roots.clone(), e, input_output, removed)
                })
                .collect();
            for e in arguments
                .iter()
                .filter(|e| matches!(e, E::ExpressionReference { .. }))
            {
                expression_targets(service, op, elements(targets.clone()), e, false, removed);
            }
            targets
        }
        E::Identity => roots,
        E::Literal { .. } => vec![],
    }
}

/// Internal permissions, aliases of them, and requirements that name them
/// are server contract only (decision 9), as internal members are. A
/// requirement the client keeps never reads an internal member: the shared
/// check refuses a client-visible clause that binds or tests one
/// (`permissions.internal-member`), so nothing here redacts paths.
fn project_permissions(result: &mut Service) {
    use crate::ir::{Mode, Requirement, Requirements, Tier};
    for resource in result.permissions.values_mut() {
        let internal: BTreeSet<String> = resource
            .permissions
            .iter()
            .filter(|p| p.tier == Tier::Internal)
            .map(|p| p.symbol.clone())
            .collect();
        resource.permissions.retain(|p| {
            p.tier != Tier::Internal
                && !p
                    .alias_of
                    .as_ref()
                    .is_some_and(|primary| internal.contains(primary))
        });
    }
    let permissions = result.permissions.clone();
    for op in &mut result.operations {
        // A pending reason is the server's migration note: the client
        // contract says nothing about the operation yet (§6.3).
        if matches!(op.requirements, Requirements::Pending { .. }) {
            op.requirements = Requirements::Undeclared;
        }
        if let Requirements::List(clauses) = &mut op.requirements {
            let visible = |clause: &Requirement| {
                permissions
                    .get(&clause.resource)
                    .is_some_and(|r| r.permission(&clause.permission).is_some())
            };
            // A `filters` clause narrows what a listing answers; it refuses
            // nothing, so leaving one out hides no requirement.
            let refusing = clauses
                .iter()
                .any(|clause| !visible(clause) && clause.mode != Mode::Filter);
            clauses.retain(visible);
            // Requirements the client may not see are still requirements:
            // an empty list would read as a model that lists none, and a
            // shorter one as everything the caller must hold.
            if refusing {
                op.requirements = if clauses.is_empty() {
                    Requirements::Internal
                } else {
                    Requirements::Partial(std::mem::take(clauses))
                };
            } else if clauses.is_empty() {
                // Only internal filters: no caller is refused for lacking a
                // permission, so to the client it requires none (what it
                // answers is narrowed, as any listing's may be). Not
                // `Undeclared`, which the shared check refuses.
                op.requirements = Requirements::None;
            }
        }
    }
    result.permissions.retain(|_, r| !r.permissions.is_empty());
    // `@permissionTypes` names its enum and the resources it excludes. A
    // resource whose permissions were all internal is gone from this view,
    // so it is no longer excluded either. The enum (still a wire enum here)
    // is this view's permission types only if both halves of check 6 still
    // hold: every value names a resource of the view, and every grantable
    // resource of the view that is not excluded has a value (one whose
    // value the projection removed as internal would have none).
    if let Some(types) = &mut result.permission_types {
        types
            .excluded
            .retain(|id| result.permissions.contains_key(id));
    }
    let names: BTreeSet<&str> = result
        .permissions
        .values()
        .map(|r| r.name.as_str())
        .collect();
    let consistent = result.permission_types.as_ref().is_some_and(|types| {
        let Some(Shape::Enum(e)) = result.shapes.get(&types.shape) else {
            return false;
        };
        let values: BTreeSet<&str> = e.values.iter().map(|v| v.value.as_str()).collect();
        values.iter().all(|value| names.contains(value))
            && result.permissions.iter().all(|(id, r)| {
                let grantable = r.permissions.iter().any(|p| {
                    p.alias_of.is_none() && matches!(p.tier, Tier::Public | Tier::PermissionOnly)
                });
                !grantable || types.excluded.contains(id) || values.contains(r.name.as_str())
            })
    });
    if !consistent {
        result.permission_types = None;
    }
}
