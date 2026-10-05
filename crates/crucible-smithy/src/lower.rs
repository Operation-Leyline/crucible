//! Lowering one service from the JSON AST to the IR.
//!
//! Crucible generates from a deliberate subset of Smithy: its own JSON
//! protocol (`smithy/crucible.smithy`), bearer auth, JSON bodies or a raw
//! blob payload. Anything outside it is a diagnostic, never a silent
//! skip — a generator that drops what it does not understand produces code
//! that quietly disagrees with the model.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::ast::{self, Kind, Traits};
use crate::diagnostics::Diagnostics;
use crate::ir::{
    Binding, Bindings, Bounds, Constraints, Enum, EnumValue, ErrorInfo, Fault, List, Map, Member,
    Operation, Paginated, Payload, Retryable, Segment, Service, Shape, Simple, SimpleKind,
    Structure, TimestampFormat, Uri,
};
use crate::shape_id::ShapeId;

const PROTOCOL: (&str, &str) = ("crucible.protocols", "json");
const BEARER: (&str, &str) = ("smithy.api", "httpBearerAuth");
const PRELUDE_AUTH: &[&str] = &[
    "httpBasicAuth",
    "httpDigestAuth",
    "httpBearerAuth",
    "httpApiKeyAuth",
];

/// Traits that change the wire format in ways crucible does not generate.
const UNSUPPORTED_TRAITS: &[&str] = &[
    "streaming",
    "requiresLength",
    "httpResponseCode",
    "httpPrefixHeaders",
    "httpQueryParams",
    "httpChecksumRequired",
    "sparse",
    "endpoint",
    "hostLabel",
    "eventHeader",
    "eventPayload",
    // A JSON member name other than the member's own; not generated yet.
    "jsonName",
];

pub(crate) fn lower(
    shapes: &BTreeMap<ShapeId, ast::Shape>,
    service_id: &ShapeId,
    infrastructure: bool,
) -> Result<Service, Diagnostics> {
    let mut lowerer = Lowerer {
        model: shapes,
        diagnostics: Diagnostics::default(),
        shapes: BTreeMap::new(),
        visiting: BTreeSet::new(),
    };
    let service = lowerer.service(service_id, infrastructure);
    if lowerer.diagnostics.is_empty() {
        Ok(service)
    } else {
        Err(lowerer.diagnostics)
    }
}

struct Lowerer<'m> {
    model: &'m BTreeMap<ShapeId, ast::Shape>,
    diagnostics: Diagnostics,
    shapes: BTreeMap<ShapeId, Shape>,
    /// Shapes being lowered further up the stack: a recursive reference to
    /// one is complete once the outer call returns.
    visiting: BTreeSet<ShapeId>,
}

impl<'m> Lowerer<'m> {
    fn service(&mut self, id: &ShapeId, infrastructure: bool) -> Service {
        let shape = &self.model[id];
        self.check_protocol(id, shape);
        let auth = self.auth(id, shape);
        let service_errors: Vec<ShapeId> = shape.errors.iter().map(|r| r.target.clone()).collect();
        let default_pagination = shape.traits.get(&smithy("paginated"));
        let mut operations: Vec<Operation> = self
            .operation_ids(shape)
            .into_iter()
            .filter_map(|(op, resource)| {
                self.operation(&op, resource, &service_errors, default_pagination)
            })
            .collect();
        operations.sort_by(|a, b| a.id.name().cmp(b.id.name()));
        let resources = if infrastructure {
            crate::resources::lower(self.model, shape, &mut self.diagnostics)
        } else {
            BTreeMap::new()
        };
        for resource in resources.values() {
            for target in resource
                .identifiers
                .values()
                .chain(resource.properties.values())
            {
                self.visit(target);
            }
        }
        let (resource_roots, resource_references) = if infrastructure {
            (
                crate::resources::sorted_refs(&shape.resources),
                crate::resources::references(self.model, self.shapes.keys(), &mut self.diagnostics),
            )
        } else {
            (Vec::new(), BTreeMap::new())
        };
        self.check_stray_payloads(&operations);
        Service {
            id: id.clone(),
            version: shape.version.clone().unwrap_or_default(),
            title: string_trait(&shape.traits, "title"),
            docs: string_trait(&shape.traits, "documentation"),
            auth,
            operations,
            resources,
            resource_roots,
            resource_references,
            shapes: std::mem::take(&mut self.shapes),
            traits: shape.traits.clone(),
            renames: shape.rename.clone(),
        }
    }

    /// The service must carry crucible's protocol. Other protocol traits may
    /// sit beside it, for other tools during a migration; crucible generates
    /// none of them.
    fn check_protocol(&mut self, id: &ShapeId, shape: &ast::Shape) {
        let json = ShapeId::new(PROTOCOL.0, PROTOCOL.1);
        if !shape.traits.contains_key(&json) {
            self.diagnostics.push(
                id,
                format!(
                    "the service must use the @{json} protocol (defined in crucible's smithy/crucible.smithy)"
                ),
            );
        }
    }

    fn auth(&mut self, id: &ShapeId, shape: &ast::Shape) -> Vec<ShapeId> {
        let bearer = ShapeId::new(BEARER.0, BEARER.1);
        let schemes: Vec<ShapeId> = shape
            .traits
            .keys()
            .filter(|t| self.is_definition(t, "authDefinition"))
            .cloned()
            .collect();
        for scheme in schemes.iter().filter(|s| **s != bearer) {
            self.diagnostics.push(
                id,
                format!("auth scheme {scheme} is not supported; only {bearer} is"),
            );
        }
        schemes
    }

    /// Whether `trait_id` is defined as a protocol or auth scheme. A built
    /// model leaves out the prelude, so its auth schemes are named here.
    fn is_definition(&self, trait_id: &ShapeId, marker: &str) -> bool {
        let prelude_auth = marker == "authDefinition"
            && trait_id.is_prelude()
            && PRELUDE_AUTH.contains(&trait_id.name());
        prelude_auth
            || self
                .model
                .get(trait_id)
                .is_some_and(|definition| definition.traits.contains_key(&smithy(marker)))
    }

    /// Every operation the service binds, with the resource that binds it
    /// (`None` for the service itself). An operation two resources bind
    /// belongs to the first one reached.
    fn operation_ids(&self, service: &ast::Shape) -> BTreeMap<ShapeId, Option<ShapeId>> {
        let mut operations: BTreeMap<ShapeId, Option<ShapeId>> = service
            .operations
            .iter()
            .map(|r| (r.target.clone(), None))
            .collect();
        let mut resources: Vec<&ShapeId> =
            service.resources.iter().rev().map(|r| &r.target).collect();
        let mut seen = BTreeSet::new();
        while let Some(id) = resources.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(resource) = self.model.get(id) else {
                continue;
            };
            let lifecycle = [
                &resource.create,
                &resource.put,
                &resource.read,
                &resource.update,
                &resource.delete,
                &resource.list,
            ];
            let bound = lifecycle
                .into_iter()
                .flatten()
                .chain(&resource.operations)
                .chain(&resource.collection_operations);
            for operation in bound {
                operations
                    .entry(operation.target.clone())
                    .or_insert_with(|| Some(id.clone()));
            }
            resources.extend(resource.resources.iter().rev().map(|r| &r.target));
        }
        operations
    }

    fn operation(
        &mut self,
        id: &ShapeId,
        resource: Option<ShapeId>,
        service_errors: &[ShapeId],
        default_pagination: Option<&Value>,
    ) -> Option<Operation> {
        let Some(shape) = self.model.get(id) else {
            self.diagnostics.push(id, "operation is not defined");
            return None;
        };
        self.check_traits(id, &shape.traits);
        let Some(http) = shape.traits.get(&smithy("http")) else {
            self.diagnostics.push(id, "operation has no @http trait");
            return None;
        };
        let method = http["method"].as_str().unwrap_or_default().to_owned();
        let uri = self.uri(id, http["uri"].as_str().unwrap_or_default());
        let status = http.get("code").and_then(Value::as_u64).unwrap_or(200);

        let input = self.io(id, shape.input.as_ref());
        let output = self.io(id, shape.output.as_ref());
        let request = input
            .as_ref()
            .map(|input| self.request_bindings(id, input, &uri))
            .unwrap_or_default();
        let response = output
            .as_ref()
            .map(|output| self.response_bindings(output))
            .unwrap_or_default();
        if input.is_none() && uri.labels().next().is_some() {
            self.diagnostics
                .push(id, "the URI has labels but the operation has no input");
        }

        let mut errors: Vec<ShapeId> = shape.errors.iter().map(|r| r.target.clone()).collect();
        for error in service_errors {
            if !errors.contains(error) {
                errors.push(error.clone());
            }
        }
        for error in &errors {
            self.visit(error);
            if !self
                .model
                .get(error)
                .is_some_and(|e| e.traits.contains_key(&smithy("error")))
            {
                self.diagnostics.push(
                    id,
                    format!("{error} is listed as an error but has no @error trait"),
                );
            }
        }

        let idempotency_token = input.as_ref().and_then(|input| {
            self.model[input]
                .members
                .iter()
                .find(|(_, member)| member.traits.contains_key(&smithy("idempotencyToken")))
                .map(|(name, _)| name.clone())
        });
        let paginated = self.paginated(id, shape, default_pagination);

        Some(Operation {
            id: id.clone(),
            resource,
            docs: string_trait(&shape.traits, "documentation"),
            method,
            uri,
            status: u16::try_from(status).unwrap_or(200),
            input,
            output,
            errors,
            readonly: shape.traits.contains_key(&smithy("readonly")),
            idempotent: shape.traits.contains_key(&smithy("idempotent")),
            idempotency_token,
            paginated,
            request,
            response,
            traits: shape.traits.clone(),
        })
    }

    /// An operation's input or output: `None` for Unit, else a structure.
    fn io(&mut self, operation: &ShapeId, io: Option<&ast::Ref>) -> Option<ShapeId> {
        let target = &io?.target;
        if *target == smithy("Unit") {
            return None;
        }
        self.visit(target);
        match self.model.get(target).map(|shape| shape.kind) {
            Some(Kind::Structure) => Some(target.clone()),
            _ => {
                self.diagnostics.push(
                    operation,
                    format!("input and output must be structures; {target} is not"),
                );
                None
            }
        }
    }

    fn uri(&mut self, operation: &ShapeId, pattern: &str) -> Uri {
        let (path, query) = pattern.split_once('?').unwrap_or((pattern, ""));
        let mut segments = Vec::new();
        for part in path.split('/').filter(|part| !part.is_empty()) {
            match part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
                Some(label) if label.ends_with('+') => {
                    self.diagnostics.push(
                        operation,
                        format!("greedy label {{{label}}} is not supported"),
                    );
                    segments.push(Segment::Label(label.trim_end_matches('+').to_owned()));
                }
                Some(label) => segments.push(Segment::Label(label.to_owned())),
                None if part.contains(['{', '}']) => {
                    self.diagnostics.push(
                        operation,
                        format!("URI segment `{part}` mixes a label with literal text"),
                    );
                }
                None => segments.push(Segment::Literal(part.to_owned())),
            }
        }
        let query = query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| match pair.split_once('=') {
                Some((key, value)) => (key.to_owned(), Some(value.to_owned())),
                None => (pair.to_owned(), None),
            })
            .collect();
        Uri { segments, query }
    }

    fn request_bindings(&mut self, operation: &ShapeId, input: &ShapeId, uri: &Uri) -> Bindings {
        let mut bindings = Bindings::default();
        for (name, member) in &self.model[input].members {
            let member_id = input.with_member(name);
            if member.traits.contains_key(&smithy("httpPayload")) {
                bindings.payload = self.payload(&member_id, member);
            } else if member.traits.contains_key(&smithy("httpLabel")) {
                if !member.traits.contains_key(&smithy("required")) {
                    self.diagnostics
                        .push(&member_id, "an @httpLabel member must be @required");
                }
                self.check_scalar(&member_id, &member.target, "@httpLabel", false);
                bindings.labels.push(name.clone());
            } else if let Some(param) = string_trait(&member.traits, "httpQuery") {
                self.check_scalar(&member_id, &member.target, "@httpQuery", true);
                bindings.query.push(Binding {
                    member: name.clone(),
                    name: param,
                });
            } else if let Some(header) = string_trait(&member.traits, "httpHeader") {
                self.check_scalar(&member_id, &member.target, "@httpHeader", true);
                bindings.headers.push(Binding {
                    member: name.clone(),
                    name: header,
                });
            } else {
                bindings.body.push(name.clone());
            }
        }
        let bound: BTreeSet<&str> = bindings.labels.iter().map(String::as_str).collect();
        let in_uri: BTreeSet<&str> = uri.labels().collect();
        for label in in_uri.difference(&bound) {
            self.diagnostics.push(
                operation,
                format!("URI label {{{label}}} has no @httpLabel member in {input}"),
            );
        }
        for label in bound.difference(&in_uri) {
            self.diagnostics.push(
                &input.with_member(label),
                "@httpLabel member does not appear in the operation's URI",
            );
        }
        self.check_alone(input, &bindings);
        bindings
    }

    fn response_bindings(&mut self, output: &ShapeId) -> Bindings {
        let mut bindings = Bindings::default();
        for (name, member) in &self.model[output].members {
            if member.traits.contains_key(&smithy("httpPayload")) {
                bindings.payload = self.payload(&output.with_member(name), member);
                continue;
            }
            match string_trait(&member.traits, "httpHeader") {
                Some(header) => {
                    self.check_scalar(
                        &output.with_member(name),
                        &member.target,
                        "@httpHeader",
                        true,
                    );
                    bindings.headers.push(Binding {
                        member: name.clone(),
                        name: header,
                    });
                }
                None => bindings.body.push(name.clone()),
            }
        }
        self.check_alone(output, &bindings);
        bindings
    }

    /// An `@httpPayload` member: the whole body, raw. crucible carries a blob
    /// there. A structure, union, string or document payload (the body as
    /// that value's JSON or text) is not generated.
    fn payload(&mut self, member_id: &ShapeId, member: &ast::Member) -> Option<Payload> {
        let target = self.model.get(&member.target)?;
        if target.kind != Kind::Blob {
            self.diagnostics.push(
                member_id,
                format!(
                    "@httpPayload must target a blob; a {} payload is not supported",
                    format!("{:?}", target.kind).to_lowercase()
                ),
            );
            return None;
        }
        let media_type = string_trait(&target.traits, "mediaType")
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        // It travels as the Content-Type header.
        if !media_type
            .bytes()
            .all(|b| b == b'\t' || (0x20..0x7f).contains(&b))
        {
            self.diagnostics.push(
                &member.target,
                format!("@mediaType {media_type:?} is not a valid header value"),
            );
        }
        Some(Payload {
            member: member_id.member().unwrap_or_default().to_owned(),
            media_type,
        })
    }

    /// A payload is the whole body: nothing else can travel in it.
    fn check_alone(&mut self, structure: &ShapeId, bindings: &Bindings) {
        let Some(payload) = &bindings.payload else {
            return;
        };
        for member in &bindings.body {
            self.diagnostics.push(
                &structure.with_member(member),
                format!(
                    "the body is the @httpPayload member `{}`; bind this member to a label, query parameter or header",
                    payload.member
                ),
            );
        }
    }

    /// `@httpPayload` binds a member of an operation's input or output; on
    /// any other structure it would be silently ignored.
    fn check_stray_payloads(&mut self, operations: &[Operation]) {
        let bound: BTreeSet<&ShapeId> = operations
            .iter()
            .flat_map(|op| op.input.iter().chain(&op.output))
            .collect();
        let stray: Vec<ShapeId> = self
            .shapes
            .iter()
            .filter(|(id, _)| !bound.contains(id))
            .filter_map(|(id, shape)| match shape {
                Shape::Structure(s) | Shape::Union(s) if s.error.is_none() => Some((id, s)),
                _ => None,
            })
            .flat_map(|(id, s)| {
                s.members
                    .iter()
                    .filter(|m| m.traits.contains_key(&smithy("httpPayload")))
                    .map(|m| id.with_member(&m.name))
            })
            .collect();
        for member in stray {
            self.diagnostics.push(
                &member,
                "@httpPayload binds only a member of an operation's input or output",
            );
        }
    }

    /// Labels, query parameters and headers carry text: a simple value, an
    /// enum, or (where `lists` allows) a list of those.
    fn check_scalar(&mut self, member: &ShapeId, target: &ShapeId, binding: &str, lists: bool) {
        let Some(shape) = self.model.get(target) else {
            return;
        };
        let scalar = |kind: Kind| {
            !matches!(
                kind,
                Kind::Structure
                    | Kind::Union
                    | Kind::Map
                    | Kind::List
                    | Kind::Set
                    | Kind::Document
                    | Kind::Blob
            )
        };
        let ok = match shape.kind {
            Kind::List if lists => shape
                .member
                .as_ref()
                .and_then(|m| self.model.get(&m.target))
                .is_some_and(|m| scalar(m.kind)),
            kind => scalar(kind),
        };
        if !ok {
            self.diagnostics
                .push(member, format!("{binding} cannot carry {target}"));
        }
    }

    fn paginated(
        &mut self,
        id: &ShapeId,
        shape: &ast::Shape,
        default: Option<&Value>,
    ) -> Option<Paginated> {
        let own = shape.traits.get(&smithy("paginated"))?;
        let field = |name: &str| {
            own.get(name)
                .or_else(|| default.and_then(|d| d.get(name)))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let (Some(input_token), Some(output_token)) = (field("inputToken"), field("outputToken"))
        else {
            self.diagnostics.push(
                id,
                "@paginated needs inputToken and outputToken (here or on the service)",
            );
            return None;
        };
        Some(Paginated {
            input_token,
            output_token,
            items: field("items"),
            page_size: field("pageSize"),
        })
    }

    fn check_traits(&mut self, id: &ShapeId, traits: &Traits) {
        for name in UNSUPPORTED_TRAITS {
            if traits.contains_key(&smithy(name)) {
                self.diagnostics
                    .push(id, format!("@{name} is not supported"));
            }
        }
    }

    fn visit(&mut self, id: &ShapeId) {
        if self.shapes.contains_key(id) || self.visiting.contains(id) {
            return;
        }
        let Some(shape) = self.model.get(id) else {
            self.diagnostics.push(id, "shape is not defined");
            return;
        };
        if !shape.mixins.is_empty() {
            self.diagnostics.push(
                id,
                "shape uses mixins; build the model with the flattenAndRemoveMixins transform",
            );
        }
        self.check_traits(id, &shape.traits);
        self.visiting.insert(id.clone());
        let lowered = self.shape(id, shape);
        self.visiting.remove(id);
        if let Some(lowered) = lowered {
            self.shapes.insert(id.clone(), lowered);
        }
    }

    fn shape(&mut self, id: &ShapeId, shape: &'m ast::Shape) -> Option<Shape> {
        let docs = string_trait(&shape.traits, "documentation");
        let traits = shape.traits.clone();
        let simple = |kind| {
            Shape::Simple(Simple {
                kind,
                constraints: constraints(&shape.traits),
                timestamp_format: timestamp_format(&shape.traits),
                docs: docs.clone(),
                traits: traits.clone(),
            })
        };
        Some(match shape.kind {
            Kind::Boolean => simple(SimpleKind::Boolean),
            Kind::String => simple(SimpleKind::String),
            Kind::Byte => simple(SimpleKind::Byte),
            Kind::Short => simple(SimpleKind::Short),
            Kind::Integer => simple(SimpleKind::Integer),
            Kind::Long => simple(SimpleKind::Long),
            Kind::Float => simple(SimpleKind::Float),
            Kind::Double => simple(SimpleKind::Double),
            Kind::Timestamp => simple(SimpleKind::Timestamp),
            Kind::Document => simple(SimpleKind::Document),
            Kind::Blob => simple(SimpleKind::Blob),
            Kind::Enum => Shape::Enum(Enum {
                values: shape
                    .members
                    .iter()
                    .map(|(name, member)| EnumValue {
                        name: name.clone(),
                        value: string_trait(&member.traits, "enumValue")
                            .unwrap_or_else(|| name.clone()),
                        docs: string_trait(&member.traits, "documentation"),
                    })
                    .collect(),
                docs,
                traits,
            }),
            Kind::List => {
                let member = self.collection_member(id, "member", shape.member.as_ref(), false)?;
                Shape::List(List {
                    member,
                    constraints: constraints(&shape.traits),
                    docs,
                    traits,
                })
            }
            Kind::Map => {
                let key = self.collection_member(id, "key", shape.key.as_ref(), false)?;
                let value = self.collection_member(id, "value", shape.value.as_ref(), false)?;
                Shape::Map(Map {
                    key,
                    value,
                    constraints: constraints(&shape.traits),
                    docs,
                    traits,
                })
            }
            Kind::Structure if shape.traits.contains_key(&smithy("unitType")) => Shape::Unit,
            Kind::Structure => Shape::Structure(Structure {
                members: self.members(id, shape),
                error: self.error_info(id, shape),
                docs,
                traits,
            }),
            Kind::Union => Shape::Union(Structure {
                members: self.members(id, shape),
                error: None,
                docs,
                traits,
            }),
            Kind::BigInteger
            | Kind::BigDecimal
            | Kind::IntEnum
            | Kind::Set
            | Kind::Service
            | Kind::Operation
            | Kind::Resource
            | Kind::Apply => {
                self.diagnostics.push(
                    id,
                    format!("{:?} shapes are not supported here", shape.kind).to_lowercase(),
                );
                return None;
            }
        })
    }

    fn collection_member(
        &mut self,
        id: &ShapeId,
        name: &str,
        member: Option<&ast::Member>,
        client_optional: bool,
    ) -> Option<Member> {
        let Some(member) = member else {
            self.diagnostics.push(id, format!("missing `{name}`"));
            return None;
        };
        Some(self.member(&id.with_member(name), name, member, client_optional))
    }

    fn members(&mut self, id: &ShapeId, shape: &ast::Shape) -> Vec<Member> {
        let input = shape.traits.contains_key(&smithy("input"));
        shape
            .members
            .iter()
            .map(|(name, member)| self.member(&id.with_member(name), name, member, input))
            .collect()
    }

    fn member(
        &mut self,
        member_id: &ShapeId,
        name: &str,
        member: &ast::Member,
        in_input: bool,
    ) -> Member {
        self.check_traits(member_id, &member.traits);
        self.visit(&member.target);
        let target_traits = self.model.get(&member.target).map(|t| &t.traits);
        let mut effective = target_traits.map(constraints).unwrap_or_default();
        let own = constraints(&member.traits);
        if member.traits.contains_key(&smithy("length")) {
            effective.length = own.length;
        }
        if member.traits.contains_key(&smithy("range")) {
            effective.range = own.range;
        }
        if own.pattern.is_some() {
            effective.pattern = own.pattern;
        }
        effective.unique_items |= own.unique_items;
        let default = member
            .traits
            .get(&smithy("default"))
            .or_else(|| target_traits.and_then(|t| t.get(&smithy("default"))))
            .filter(|value| !value.is_null())
            .cloned();
        Member {
            name: name.to_owned(),
            target: member.target.clone(),
            required: member.traits.contains_key(&smithy("required")),
            default,
            client_optional: in_input || member.traits.contains_key(&smithy("clientOptional")),
            constraints: effective,
            timestamp_format: timestamp_format(&member.traits),
            docs: string_trait(&member.traits, "documentation"),
            traits: member.traits.clone(),
        }
    }

    fn error_info(&mut self, id: &ShapeId, shape: &ast::Shape) -> Option<ErrorInfo> {
        let fault = match shape.traits.get(&smithy("error"))?.as_str() {
            Some("client") => Fault::Client,
            Some("server") => Fault::Server,
            other => {
                self.diagnostics.push(
                    id,
                    format!("@error must be client or server, not {other:?}"),
                );
                return None;
            }
        };
        let status = shape
            .traits
            .get(&smithy("httpError"))
            .and_then(Value::as_u64)
            .and_then(|code| u16::try_from(code).ok())
            .unwrap_or(match fault {
                Fault::Client => 400,
                Fault::Server => 500,
            });
        let retryable = shape
            .traits
            .get(&smithy("retryable"))
            .map(|value| Retryable {
                throttling: value
                    .get("throttling")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        let mut headers = Vec::new();
        for (name, member) in &shape.members {
            if member.traits.contains_key(&smithy("httpPayload")) {
                // Clients name an error by its JSON body's `__type`.
                self.diagnostics.push(
                    &id.with_member(name),
                    "@httpPayload is not supported on an error: its body is JSON naming the error",
                );
            }
            if let Some(header) = string_trait(&member.traits, "httpHeader") {
                self.check_scalar(&id.with_member(name), &member.target, "@httpHeader", true);
                headers.push(Binding {
                    member: name.clone(),
                    name: header,
                });
            }
        }
        Some(ErrorInfo {
            fault,
            status,
            retryable,
            headers,
        })
    }
}

fn smithy(name: &str) -> ShapeId {
    ShapeId::new("smithy.api", name)
}

fn string_trait(traits: &Traits, name: &str) -> Option<String> {
    traits.get(&smithy(name))?.as_str().map(str::to_owned)
}

fn constraints(traits: &Traits) -> Constraints {
    let bounds = |name: &str| {
        traits.get(&smithy(name)).map(|value| {
            (
                value.get("min").and_then(Value::as_number).cloned(),
                value.get("max").and_then(Value::as_number).cloned(),
            )
        })
    };
    Constraints {
        length: bounds("length").map(|(min, max)| Bounds {
            min: min.and_then(|n| n.as_u64()),
            max: max.and_then(|n| n.as_u64()),
        }),
        range: bounds("range").map(|(min, max)| Bounds { min, max }),
        pattern: string_trait(traits, "pattern"),
        unique_items: traits.contains_key(&smithy("uniqueItems")),
    }
}

fn timestamp_format(traits: &Traits) -> Option<TimestampFormat> {
    match string_trait(traits, "timestampFormat")?.as_str() {
        "date-time" => Some(TimestampFormat::DateTime),
        "http-date" => Some(TimestampFormat::HttpDate),
        "epoch-seconds" => Some(TimestampFormat::EpochSeconds),
        _ => None,
    }
}
