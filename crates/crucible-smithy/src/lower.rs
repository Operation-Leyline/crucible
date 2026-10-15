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
    Binding, Bindings, Bounds, Constraints, DocNotes, Enum, EnumValue, ErrorInfo, Fault,
    HeaderEncoding, IntEnum, IntEnumValue, List, Map, Member, Mutability, Operation, Paginated,
    Payload, PayloadKind, Retryable, Segment, Service, Shape, Simple, SimpleKind, Structure,
    TimestampFormat, Uri,
};
use crate::shape_id::ShapeId;
use crate::{Capabilities, Feature, FeatureUse, Protocol};

/// `@auth([])`: declared public, as opposed to having no scheme to inherit.
fn explicitly_public(traits: &Traits) -> bool {
    traits
        .get(&smithy("auth"))
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
}

/// The protocols Crucible no longer serves. Matched by name as well as by
/// `@protocolDefinition`, because a built model leaves out the prelude.
const REFUSED_PROTOCOLS: &[(&str, &str)] = &[
    ("aws.protocols", "restJson1"),
    ("aws.protocols", "awsJson1_0"),
    ("aws.protocols", "awsJson1_1"),
    ("aws.protocols", "restXml"),
    ("aws.protocols", "awsQuery"),
    ("aws.protocols", "ec2Query"),
    ("smithy.protocols", "rpcv2Cbor"),
    ("smithy.protocols", "rpcv2Json"),
];

/// Traits that only XML and query protocols give meaning to.
const REFUSED_XML_AND_QUERY_TRAITS: &[(&str, &str)] = &[
    ("smithy.api", "xmlName"),
    ("smithy.api", "xmlFlattened"),
    ("smithy.api", "xmlAttribute"),
    ("smithy.api", "xmlNamespace"),
    ("aws.protocols", "awsQueryCompatible"),
    ("aws.protocols", "awsQueryError"),
    ("aws.protocols", "ec2QueryName"),
    ("aws.customizations", "s3UnwrappedXmlOutput"),
];

/// The typed permission traits (`crucible.auth#<name>`), which only `.cru`
/// models declare (docs/permissions.md §4.11). `crucible-traits` no longer
/// defines them, so a Smithy model that uses one is refused: here at
/// lowering, or by the Smithy compiler first (`Model.UnresolvedTrait`),
/// whose failure `crucible-build` explains with [`permission_trait_refusal`].
pub const PERMISSION_TRAITS: &[&str] = &[
    "permissionsOf",
    "authorizationName",
    "permissionOnly",
    "notGrantable",
    "aliasOf",
    "permissionTypes",
    "requires",
    "requiresNone",
    "requiresPending",
];

/// Why a Smithy model may not use the permission trait `name`, one of
/// [`PERMISSION_TRAITS`], and what to do instead.
pub fn permission_trait_refusal(name: &str) -> String {
    format!(
        "@crucible.auth#{name} is not supported: typed permissions are declared in .cru models (docs/permissions.md); migrate the model to .cru to declare them"
    )
}

/// Smithy features Crucible does not implement. A model that uses one is
/// refused, never lowered as if it did not: streaming payloads and event
/// streams, waiters, endpoint rules and discovery, host prefixes, checksums,
/// non-bearer auth, CORS, and the HTTP bindings and naming only those used.
const REFUSED_TRAITS: &[(&str, &str)] = &[
    ("smithy.api", "enum"),
    ("smithy.api", "streaming"),
    ("smithy.api", "requiresLength"),
    ("smithy.api", "eventHeader"),
    ("smithy.api", "eventPayload"),
    ("smithy.api", "jsonName"),
    ("smithy.api", "httpPrefixHeaders"),
    ("smithy.api", "httpQueryParams"),
    ("smithy.api", "httpResponseCode"),
    ("smithy.api", "optionalAuth"),
    ("smithy.api", "hostLabel"),
    ("smithy.api", "endpoint"),
    ("smithy.api", "httpChecksumRequired"),
    ("smithy.api", "cors"),
    ("aws.protocols", "httpChecksum"),
    ("aws.auth", "unsignedPayload"),
    ("aws.api", "clientEndpointDiscovery"),
    ("aws.api", "clientDiscoveredEndpoint"),
    ("aws.api", "clientEndpointDiscoveryId"),
    ("smithy.waiters", "waitable"),
    ("smithy.rules", "endpointRuleSet"),
    ("smithy.rules", "clientContextParams"),
    ("smithy.rules", "staticContextParams"),
    ("smithy.rules", "contextParam"),
    ("smithy.rules", "operationContextParams"),
];

const PRELUDE_AUTH: &[&str] = &[
    "httpBasicAuth",
    "httpDigestAuth",
    "httpBearerAuth",
    "httpApiKeyAuth",
];

pub(crate) fn lower(
    shapes: &BTreeMap<ShapeId, ast::Shape>,
    service_id: &ShapeId,
    capabilities: &Capabilities,
    infrastructure: bool,
    documentation: bool,
) -> Result<Service, Diagnostics> {
    let mut lowerer = Lowerer {
        model: shapes,
        protocol: Protocol::CrucibleJson,
        default_scopes: Vec::new(),
        diagnostics: Diagnostics::default(),
        shapes: BTreeMap::new(),
        visiting: BTreeSet::new(),
        capabilities,
        requirements: BTreeSet::new(),
        http_inputs: BTreeSet::new(),
        http_outputs: BTreeSet::new(),
        service_public: false,
    };
    let service = lowerer.service(service_id, infrastructure, documentation);
    if lowerer.diagnostics.is_empty() {
        Ok(service)
    } else {
        Err(lowerer.diagnostics)
    }
}

pub(crate) struct Lowerer<'m> {
    protocol: Protocol,
    /// The service's `@crucible.auth#scopes`, which operations inherit.
    default_scopes: Vec<String>,
    pub(crate) model: &'m BTreeMap<ShapeId, ast::Shape>,
    pub(crate) diagnostics: Diagnostics,
    pub(crate) shapes: BTreeMap<ShapeId, Shape>,
    /// Shapes being lowered further up the stack: a recursive reference to
    /// one is complete once the outer call returns.
    visiting: BTreeSet<ShapeId>,
    capabilities: &'m Capabilities,
    requirements: BTreeSet<FeatureUse>,
    http_inputs: BTreeSet<ShapeId>,
    http_outputs: BTreeSet<ShapeId>,
    /// The service's own `@auth([])`, which operations without an `@auth`
    /// inherit as an explicit public declaration.
    service_public: bool,
}

impl<'m> Lowerer<'m> {
    pub(crate) fn reached_ids(&self) -> impl Iterator<Item = ShapeId> + '_ {
        self.shapes.keys().cloned()
    }
    fn explicit_timestamp_format(&self, traits: &Traits) -> Option<TimestampFormat> {
        timestamp_format(traits)
    }
    fn service(&mut self, id: &ShapeId, infrastructure: bool, documentation: bool) -> Service {
        let shape = &self.model[id];
        self.check_protocol(id, shape);
        self.check_traits(id, &shape.traits);
        let auth = self.auth(id, shape);
        let auth_schemes = self.auth_configs(id, &auth);
        let default_auth = self.effective_auth(id, &shape.traits, &auth);
        self.service_public = explicitly_public(&shape.traits);
        self.refuse_permission_traits();
        let scopes = crate::scopes::lower(id, &shape.traits, &mut self.diagnostics);
        if scopes.as_ref().is_some_and(|scopes| !scopes.is_empty()) && default_auth.is_empty() {
            self.diagnostics.push(
                id,
                "@crucible.auth#scopes requires the service to declare an authentication scheme",
            );
        }
        self.default_scopes = scopes.unwrap_or_default();
        let service_errors: Vec<ShapeId> = shape.errors.iter().map(|r| r.target.clone()).collect();
        let default_pagination = shape.traits.get(&smithy("paginated"));
        for (operation, _) in self.operation_ids(shape) {
            if let Some(operation) = self.model.get(&operation) {
                self.http_inputs
                    .extend(operation.input.iter().map(|r| r.target.clone()));
                self.http_outputs.extend(
                    operation
                        .output
                        .iter()
                        .chain(&operation.errors)
                        .map(|r| r.target.clone()),
                );
            }
        }
        self.http_outputs.extend(service_errors.iter().cloned());
        let mut operations: Vec<Operation> = self
            .operation_ids(shape)
            .into_iter()
            .filter_map(|(op, resource)| {
                self.operation(
                    &op,
                    resource,
                    &service_errors,
                    default_pagination,
                    &default_auth,
                    &auth,
                )
            })
            .collect();
        // Service errors remain reachable before the service has any operations.
        for error in &service_errors {
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
        operations.sort_by(|a, b| a.id.name().cmp(b.id.name()));
        let resources = if infrastructure {
            crate::resources::lower(self.model, shape, &mut self.diagnostics)
        } else if documentation {
            crate::resources::lower_for_output(self.model, shape, &mut self.diagnostics, false)
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
        let (resource_roots, resource_references) = if infrastructure || documentation {
            (
                crate::resources::sorted_refs(&shape.resources),
                if infrastructure {
                    crate::resources::references(
                        self.model,
                        self.shapes.keys(),
                        &mut self.diagnostics,
                    )
                } else {
                    BTreeMap::new()
                },
            )
        } else {
            (Vec::new(), BTreeMap::new())
        };
        let reached: Vec<_> = self.reached_ids().collect();
        let recursive_groups = crate::recursion::lower(self.model, &reached);
        let service = Service {
            internal: shape.traits.contains_key(&smithy("internal")),
            recursive_groups,
            protocol: self.protocol,
            errors: service_errors,
            protocol_settings: self.protocol_settings(id, shape),
            requirements: std::mem::take(&mut self.requirements).into_iter().collect(),
            id: id.clone(),
            version: shape.version.clone().unwrap_or_default(),
            title: string_trait(&shape.traits, "title"),
            docs: string_trait(&shape.traits, "documentation"),
            notes: DocNotes::lower(id, &shape.traits, &mut self.diagnostics),
            auth,
            scopes: std::mem::take(&mut self.default_scopes),
            auth_schemes,
            operations,
            resources,
            resource_roots,
            resource_references,
            shapes: std::mem::take(&mut self.shapes),
            traits: shape.traits.clone(),
            renames: shape.rename.clone(),
            versions: Vec::new(),
            api_version: None,
            internal_traits: self
                .model
                .iter()
                .filter(|(_, definition)| {
                    definition.traits.contains_key(&smithy("trait"))
                        && definition.traits.contains_key(&smithy("internal"))
                })
                .map(|(id, _)| id.clone())
                .collect(),
            // Typed permissions are declared in .cru only.
            permissions: BTreeMap::new(),
            permission_types: None,
            not_found: None,
            exceptions: Vec::new(),
        };
        // Rules of the IR itself, shared with the .cru front end.
        crate::validate::service(&service, &mut self.diagnostics);
        service
    }

    /// Crucible's JSON protocol is the only wire protocol. A model that
    /// declares any other is refused rather than lowered as if it used JSON.
    fn check_protocol(&mut self, id: &ShapeId, shape: &ast::Shape) {
        let mut declared_json = false;
        for trait_id in shape.traits.keys() {
            if *trait_id == Protocol::CrucibleJson.trait_id() {
                declared_json = true;
            } else if REFUSED_PROTOCOLS.contains(&(trait_id.namespace(), trait_id.name()))
                || self.is_definition(trait_id, "protocolDefinition")
            {
                self.diagnostics
                    .push(id, format!("protocol {trait_id} is not supported; the service must use @crucible.protocols#json"));
            }
        }
        if !declared_json {
            self.diagnostics.push(
                id,
                "the service must use the @crucible.protocols#json protocol",
            );
        }
        self.protocol = Protocol::CrucibleJson;
    }

    fn protocol_settings(&mut self, id: &ShapeId, shape: &ast::Shape) -> crate::ProtocolSettings {
        let mut settings = crate::ProtocolSettings::default();
        let trait_id = self.protocol.trait_id();
        let Some(value) = shape.traits.get(&trait_id) else {
            return settings;
        };
        let Some(value) = value.as_object() else {
            self.diagnostics
                .push(id, format!("@{trait_id} must be an object"));
            return settings;
        };
        for (key, value) in value {
            match key.as_str() {
                "http" => {
                    let versions = value.as_array().and_then(|values| {
                        values
                            .iter()
                            .map(|v| v.as_str().filter(|v| !v.is_empty()).map(str::to_owned))
                            .collect::<Option<Vec<_>>>()
                    });
                    if let Some(versions) = versions.filter(|versions| !versions.is_empty()) {
                        settings.http = versions;
                    } else {
                        self.diagnostics.push(id,format!("@{trait_id}.{key} must be a nonempty array of ALPN protocol strings"));
                    }
                }
                _ => self
                    .diagnostics
                    .push(id, format!("unsupported @{trait_id} property {key}")),
            }
        }
        settings
    }

    fn auth(&mut self, _id: &ShapeId, shape: &ast::Shape) -> Vec<ShapeId> {
        let schemes: Vec<ShapeId> = shape
            .traits
            .keys()
            .filter(|t| self.is_definition(t, "authDefinition"))
            .cloned()
            .collect();
        schemes
    }

    /// Whether `trait_id` is defined as a protocol or auth scheme. A built
    /// model leaves out the prelude, so its auth schemes are named here.
    fn is_definition(&self, trait_id: &ShapeId, marker: &str) -> bool {
        let prelude_auth = marker == "authDefinition"
            && trait_id.is_prelude()
            && PRELUDE_AUTH.contains(&trait_id.name());
        prelude_auth
            || marker == "authDefinition"
                && trait_id.namespace() == "aws.auth"
                && matches!(trait_id.name(), "sigv4" | "sigv4a")
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
        default_auth: &[ShapeId],
        supported_auth: &[ShapeId],
    ) -> Option<Operation> {
        let Some(shape) = self.model.get(id) else {
            self.diagnostics.push(id, "operation is not defined");
            return None;
        };
        self.check_traits(id, &shape.traits);
        let (auth, public) = if shape.traits.contains_key(&smithy("auth")) {
            (
                self.effective_auth(id, &shape.traits, supported_auth),
                explicitly_public(&shape.traits),
            )
        } else {
            (default_auth.to_vec(), self.service_public)
        };
        let scopes = match crate::scopes::lower(id, &shape.traits, &mut self.diagnostics) {
            // A public operation has no caller to hold a scope. A service default
            // is simply not inherited; an explicit list is a contradiction.
            Some(scopes) if auth.is_empty() => {
                if !scopes.is_empty() {
                    self.diagnostics.push(
                        id,
                        "@crucible.auth#scopes requires an authenticated operation; this one has no effective authentication scheme",
                    );
                }
                Vec::new()
            }
            Some(scopes) => scopes,
            None if auth.is_empty() => Vec::new(),
            None => self.default_scopes.clone(),
        };
        let (method, uri, status) = {
            let Some(http) = shape.traits.get(&smithy("http")) else {
                self.diagnostics.push(id, "operation has no @http trait");
                return None;
            };
            if http
                .get("method")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                self.diagnostics.push(id, "@http requires an HTTP method");
            }
            if let Some(code) = http.get("code")
                && code
                    .as_u64()
                    .is_none_or(|code| !(100..=999).contains(&code))
            {
                self.diagnostics.push(
                    id,
                    "HTTP status code must be an integer between 100 and 999",
                );
            }
            (
                http["method"].as_str().unwrap_or_default().to_owned(),
                self.uri(id, http["uri"].as_str().unwrap_or_default()),
                http.get("code").and_then(Value::as_u64).unwrap_or(200),
            )
        };

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
        let mut protocol_headers = BTreeMap::new();
        if let Some(payload) = &request.payload {
            protocol_headers.insert("Content-Type".into(), payload.media_type.clone());
        } else if !request.body.is_empty() {
            protocol_headers.insert("Content-Type".into(), "application/json".into());
        }
        if request
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("Content-Type"))
        {
            protocol_headers.remove("Content-Type");
        }
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
        let examples = self.examples(id, shape, &errors);

        Some(Operation {
            protocol_headers,
            response_protocol_headers: self.protocol.response_headers(
                &response,
                output.is_some() && (!response.body.is_empty() || response.payload.is_some()),
            ),
            internal: shape.traits.contains_key(&smithy("internal")),
            id: id.clone(),
            auth,
            public,
            scopes,
            resource,
            docs: string_trait(&shape.traits, "documentation"),
            notes: DocNotes::lower(id, &shape.traits, &mut self.diagnostics),
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
            examples,
            request,
            response,
            traits: shape.traits.clone(),
            requirements: Default::default(),
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
        if !path.starts_with('/') {
            self.diagnostics.push(operation, "URI must start with /");
        }
        if path.contains("//") {
            self.diagnostics
                .push(operation, "URI must not contain empty path segments");
        }
        if query.contains(['{', '}']) {
            self.diagnostics
                .push(operation, "URI query string cannot contain labels");
        }
        let mut segments = Vec::new();
        let mut greedy_labels = Vec::new();
        for part in path.split('/').filter(|part| !part.is_empty()) {
            match part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
                Some(label) if label.ends_with('+') => {
                    self.require(operation, Feature::GreedyLabels, smithy("http"));
                    greedy_labels.push(label.trim_end_matches('+').to_owned());
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
        let mut labels = BTreeSet::new();
        for segment in &segments {
            if let Segment::Label(name) = segment
                && !labels.insert(name)
            {
                self.diagnostics
                    .push(operation, format!("duplicate URI label {{{name}}}"));
            }
        }
        if greedy_labels.len() > 1 {
            self.diagnostics.push(
                operation,
                "a URI pattern may contain at most one greedy label",
            );
        }
        let query = query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| match pair.split_once('=') {
                Some((key, value)) => (key.to_owned(), Some(value.to_owned())),
                None => (pair.to_owned(), None),
            })
            .collect();
        Uri {
            segments,
            query,
            greedy_labels,
        }
    }

    fn request_bindings(&mut self, operation: &ShapeId, input: &ShapeId, uri: &Uri) -> Bindings {
        let mut bindings = Bindings::default();
        let mut seen_payload = false;
        for (name, member) in &self.model[input].members {
            let member_id = input.with_member(name);
            if member.traits.contains_key(&smithy("httpPayload")) {
                if seen_payload {
                    self.diagnostics
                        .push(&member_id, "only one member may bind @httpPayload");
                } else {
                    bindings.payload = self.payload(&member_id, member);
                }
                seen_payload = true;
            } else if member.traits.contains_key(&smithy("httpLabel")) {
                // Requiredness and target kinds are checked by crate::validate.
                bindings.labels.push(name.clone());
            } else if let Some(param) = string_trait(&member.traits, "httpQuery") {
                bindings.query.push(Binding {
                    member: name.clone(),
                    name: param,
                });
            } else if let Some(header) = string_trait(&member.traits, "httpHeader") {
                self.header_features(&member_id, &member.target);
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
        self.header_encodings(input, &mut bindings);
        self.binding_timestamp_formats(input, &mut bindings);
        bindings
    }

    fn response_bindings(&mut self, output: &ShapeId) -> Bindings {
        let mut bindings = Bindings::default();
        let mut seen_payload = false;
        for (name, member) in &self.model[output].members {
            if member.traits.contains_key(&smithy("httpPayload")) {
                if seen_payload {
                    self.diagnostics.push(
                        &output.with_member(name),
                        "only one member may bind @httpPayload",
                    );
                } else {
                    bindings.payload = self.payload(&output.with_member(name), member);
                }
                seen_payload = true;
                continue;
            }
            match string_trait(&member.traits, "httpHeader") {
                Some(header) => {
                    self.header_features(&output.with_member(name), &member.target);
                    bindings.headers.push(Binding {
                        member: name.clone(),
                        name: header,
                    });
                }
                None => bindings.body.push(name.clone()),
            }
        }
        self.header_encodings(output, &mut bindings);
        self.binding_timestamp_formats(output, &mut bindings);
        bindings
    }

    /// Resolve the representation and content type of the complete HTTP body.
    fn payload(&mut self, member_id: &ShapeId, member: &ast::Member) -> Option<Payload> {
        let target = self.model.get(&member.target)?;
        let kind = match target.kind {
            Kind::Blob => PayloadKind::Blob,
            Kind::String | Kind::Enum => PayloadKind::String,
            Kind::Structure | Kind::Union | Kind::Document => PayloadKind::Document,
            _ => {
                self.diagnostics.push(
                    member_id,
                    "@httpPayload must target a blob, string, structure, union, or document",
                );
                return None;
            }
        };
        if kind != PayloadKind::Blob {
            self.require(member_id, Feature::NonBlobPayload, smithy("httpPayload"));
        }
        let media_type = string_trait(&target.traits, "mediaType").unwrap_or_else(|| {
            match kind {
                PayloadKind::Blob => "application/octet-stream",
                PayloadKind::String => "text/plain",
                _ => "application/json",
            }
            .to_owned()
        });
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
            kind,
            member: member_id.member().unwrap_or_default().to_owned(),
            media_type,
        })
    }

    /// Labels, query parameters and headers carry text: a simple value, an
    /// enum, or (where `lists` allows) a list of those.
    fn binding_timestamp_formats(&self, structure: &ShapeId, bindings: &mut Bindings) {
        for (name, header) in bindings
            .headers
            .iter()
            .map(|b| (&b.member, true))
            .chain(bindings.query.iter().map(|b| (&b.member, false)))
            .chain(bindings.labels.iter().map(|name| (name, false)))
        {
            let Some(mut member) = self.model[structure].members.get(name) else {
                continue;
            };
            let Some(mut target) = self.model.get(&member.target) else {
                continue;
            };
            if target.kind == Kind::List {
                let Some(inner) = target.member.as_ref() else {
                    continue;
                };
                member = inner;
                let Some(inner) = self.model.get(&member.target) else {
                    continue;
                };
                target = inner;
            }
            if target.kind != Kind::Timestamp {
                continue;
            }
            let explicit = self
                .explicit_timestamp_format(&member.traits)
                .or_else(|| self.explicit_timestamp_format(&target.traits));
            let document = explicit.unwrap_or_else(|| self.protocol.timestamp_format(false));
            let binding = explicit.unwrap_or_else(|| self.protocol.timestamp_format(header));
            if binding != document {
                bindings.timestamp_formats.insert(name.clone(), binding);
            }
        }
    }

    fn header_encodings(&mut self, structure: &ShapeId, bindings: &mut Bindings) {
        for binding in bindings.headers.iter() {
            let Some(member) = self.model[structure].members.get(&binding.member) else {
                continue;
            };
            let Some(mut target) = self.model.get(&member.target) else {
                continue;
            };
            if target.kind == Kind::Map {
                let Some(value) = target
                    .value
                    .as_ref()
                    .and_then(|value| self.model.get(&value.target))
                else {
                    continue;
                };
                target = value;
            }
            if target.kind == Kind::List {
                let Some(value) = target
                    .member
                    .as_ref()
                    .and_then(|value| self.model.get(&value.target))
                else {
                    continue;
                };
                target = value;
            }
            if matches!(target.kind, Kind::String | Kind::Enum)
                && target.traits.contains_key(&smithy("mediaType"))
            {
                self.require(
                    &structure.with_member(&binding.member),
                    Feature::MediaTypeHeaders,
                    smithy("mediaType"),
                );
                bindings
                    .header_encodings
                    .insert(binding.member.clone(), HeaderEncoding::Base64);
            }
        }
    }

    /// A header carrying a media-typed string travels base64-encoded, which
    /// generators must support. What a header may carry at all is checked by
    /// `crate::validate`.
    fn header_features(&mut self, member: &ShapeId, target: &ShapeId) {
        let Some(shape) = self.model.get(target) else {
            return;
        };
        let header_target = if shape.kind == Kind::List {
            shape
                .member
                .as_ref()
                .and_then(|m| self.model.get(&m.target))
        } else {
            Some(shape)
        };
        if header_target.is_some_and(|s| {
            matches!(s.kind, Kind::String | Kind::Enum)
                && s.traits.contains_key(&smithy("mediaType"))
        }) {
            self.require(member, Feature::MediaTypeHeaders, smithy("mediaType"));
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
        let input_token = field("inputToken");
        let output_token = field("outputToken");
        let items = field("items");
        let page_size = field("pageSize");
        // Paths, member types and requiredness are checked by
        // crate::validate against the lowered shapes.
        for (name, path) in [
            ("inputToken", input_token.as_deref()),
            ("outputToken", output_token.as_deref()),
            ("items", items.as_deref()),
            ("pageSize", page_size.as_deref()),
        ] {
            if path.is_none()
                && (own
                    .get(name)
                    .or_else(|| default.and_then(|d| d.get(name)))
                    .is_some()
                    || matches!(name, "inputToken" | "outputToken"))
            {
                self.diagnostics.push(
                    id,
                    format!("@paginated {name} must be a member name or path"),
                );
            }
        }
        Some(Paginated {
            input_token: input_token?,
            output_token: output_token?,
            items,
            page_size,
        })
    }

    /// Typed permissions are a `.cru` feature: a Smithy model that uses one
    /// of their traits is refused rather than lowered as if it had none.
    fn refuse_permission_traits(&mut self) {
        let model = self.model;
        for (id, shape) in model {
            let members = shape
                .members
                .iter()
                .map(|(name, member)| (id.with_member(name), &member.traits));
            for (at, traits) in std::iter::once((id.clone(), &shape.traits)).chain(members) {
                for name in PERMISSION_TRAITS {
                    if traits.contains_key(&ShapeId::new("crucible.auth", name)) {
                        self.diagnostics.push(&at, permission_trait_refusal(name));
                    }
                }
            }
        }
    }

    /// Refuses the traits only XML and query protocols give meaning to.
    fn refuse_xml_and_query(&mut self, id: &ShapeId, traits: &Traits) {
        for (namespace, name) in REFUSED_XML_AND_QUERY_TRAITS {
            let trait_id = ShapeId::new(namespace, name);
            if traits.contains_key(&trait_id) {
                self.diagnostics.push(
                    id,
                    format!("@{trait_id} is not supported: XML and query serialization are gone"),
                );
            }
        }
    }

    fn check_traits(&mut self, id: &ShapeId, traits: &Traits) {
        for (namespace, name) in REFUSED_TRAITS {
            let trait_id = ShapeId::new(namespace, name);
            if traits.contains_key(&trait_id) {
                self.diagnostics
                    .push(id, format!("@{trait_id} is not supported"));
            }
        }
        for (name, reason) in [
            ("longPoll", "long-poll timing and timeout behavior"),
            ("createsResources", "resource lifecycle effects"),
            ("putsResources", "resource lifecycle effects"),
            ("readsResources", "resource lifecycle effects"),
            ("updatesResources", "resource lifecycle effects"),
            ("deletesResources", "resource lifecycle effects"),
        ] {
            if traits.contains_key(&smithy(name)) {
                self.diagnostics.push(
                    id,
                    format!("@smithy.api#{name} is not supported: {reason} are not implemented"),
                );
            }
        }
        if let Some(value) = traits.get(&smithy("idempotent")) {
            for member in ["exists", "notFound"] {
                if value.get(member).is_some() {
                    self.diagnostics.push(id, format!("@smithy.api#idempotent {member} is not supported: resource-existence response semantics are not implemented"));
                }
            }
        }
        for (namespace, name, reason) in [
            ("smithy.rules", "endpointTests", "endpoint test execution"),
            ("smithy.test", "smokeTests", "smoke test execution"),
        ] {
            let trait_id = ShapeId::new(namespace, name);
            if traits.contains_key(&trait_id) {
                self.diagnostics.push(
                    id,
                    format!("@{trait_id} is not supported: {reason} is not implemented"),
                );
            }
        }
        if traits.contains_key(&smithy("requestCompression")) {
            self.diagnostics
                .push(id, "@requestCompression is not supported");
        }
        self.refuse_xml_and_query(id, traits);
        // Behavior traits outside smithy.api must not silently disappear into raw traits.
        {
            let trait_id = ShapeId::new("smithy.rules", "endpointBdd");
            if traits.contains_key(&trait_id) {
                self.diagnostics
                    .push(id, format!("@{trait_id} is not supported"));
            }
        }
    }

    pub(crate) fn require(&mut self, shape: &ShapeId, feature: Feature, source: ShapeId) {
        if self.requirements.insert(FeatureUse {
            feature,
            shape: shape.clone(),
            source: source.clone(),
        }) && !self.capabilities.supports(feature)
        {
            self.diagnostics.push(
                shape,
                format!("@{source} is not supported by this target (requires {feature})"),
            );
        }
    }

    fn effective_auth(
        &mut self,
        id: &ShapeId,
        traits: &Traits,
        supported: &[ShapeId],
    ) -> Vec<ShapeId> {
        let Some(value) = traits.get(&smithy("auth")) else {
            return supported.to_vec();
        };
        let Some(values) = value.as_array() else {
            self.diagnostics
                .push(id, "@auth must be a list of authentication scheme IDs");
            return Vec::new();
        };
        values
            .iter()
            .filter_map(|value| {
                let scheme = value.as_str().and_then(|s| s.parse::<ShapeId>().ok());
                match scheme {
                    Some(scheme) if supported.contains(&scheme) => Some(scheme),
                    _ => {
                        self.diagnostics.push(
                            id,
                            format!("@auth scheme {value} is not declared on the service"),
                        );
                        None
                    }
                }
            })
            .collect()
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
        let notes = DocNotes::lower(id, &shape.traits, &mut self.diagnostics);
        let traits = shape.traits.clone();
        let explicit_format = self.explicit_timestamp_format(&shape.traits);
        let simple = |kind| {
            Shape::Simple(Simple {
                internal: shape.traits.contains_key(&smithy("internal")),
                sensitive: shape.traits.contains_key(&smithy("sensitive")),
                kind,
                constraints: constraints(&shape.traits),
                timestamp_format: explicit_format,
                docs: docs.clone(),
                notes: notes.clone(),
                traits: traits.clone(),
            })
        };
        if matches!(shape.kind, Kind::Enum | Kind::IntEnum) {
            for (name, member) in &shape.members {
                self.refuse_xml_and_query(&id.with_member(name), &member.traits);
            }
        }
        Some(match shape.kind {
            Kind::Boolean => simple(SimpleKind::Boolean),
            Kind::String => simple(SimpleKind::String),
            Kind::Byte => simple(SimpleKind::Byte),
            Kind::Short => simple(SimpleKind::Short),
            Kind::Integer => simple(SimpleKind::Integer),
            Kind::Long => simple(SimpleKind::Long),
            Kind::Float => simple(SimpleKind::Float),
            Kind::Double => simple(SimpleKind::Double),
            Kind::BigInteger => {
                self.require(id, Feature::BigInteger, smithy("BigInteger"));
                simple(SimpleKind::BigInteger)
            }
            Kind::BigDecimal => {
                self.require(id, Feature::BigDecimal, smithy("BigDecimal"));
                simple(SimpleKind::BigDecimal)
            }
            Kind::Timestamp => simple(SimpleKind::Timestamp),
            Kind::Document => simple(SimpleKind::Document),
            Kind::Blob => simple(SimpleKind::Blob),
            Kind::Enum => Shape::Enum(Enum {
                internal: shape.traits.contains_key(&smithy("internal")),
                sensitive: shape.traits.contains_key(&smithy("sensitive")),
                values: shape
                    .members
                    .iter()
                    .map(|(name, member)| EnumValue {
                        internal: member.traits.contains_key(&smithy("internal")),
                        name: name.clone(),
                        value: string_trait(&member.traits, "enumValue")
                            .unwrap_or_else(|| name.clone()),
                        docs: string_trait(&member.traits, "documentation"),
                        notes: DocNotes::lower(
                            &id.with_member(name),
                            &member.traits,
                            &mut self.diagnostics,
                        ),
                        traits: member.traits.clone(),
                    })
                    .collect(),
                docs,
                notes,
                traits,
            }),
            Kind::IntEnum => {
                self.require(id, Feature::IntEnum, smithy("IntEnum"));
                let mut values = Vec::new();
                let mut numbers = BTreeSet::new();
                for (name, member) in &shape.members {
                    let value = member
                        .traits
                        .get(&smithy("enumValue"))
                        .and_then(Value::as_i64)
                        .and_then(|value| i32::try_from(value).ok());
                    match value {
                        Some(value) => {
                            if !numbers.insert(value) {
                                self.diagnostics.push(&id.with_member(name), "intEnum values must be unique");
                            }
                            values.push(IntEnumValue {
                                internal: member.traits.contains_key(&smithy("internal")),
                                name: name.clone(), value,
                                docs: string_trait(&member.traits, "documentation"),
                                notes: DocNotes::lower(
                                    &id.with_member(name),
                                    &member.traits,
                                    &mut self.diagnostics,
                                ),
                                traits: member.traits.clone(),
                            });
                        }
                        None => self.diagnostics.push(&id.with_member(name), "intEnum member must have an integer @enumValue within signed 32-bit range"),
                    }
                }
                Shape::IntEnum(IntEnum {
                    internal: shape.traits.contains_key(&smithy("internal")),
                    sensitive: shape.traits.contains_key(&smithy("sensitive")),
                    values,
                    docs,
                    notes,
                    traits,
                })
            }
            Kind::List | Kind::Set => {
                if shape.kind == Kind::Set {
                    self.diagnostics.push(
                        id,
                        "set shapes are not supported; use a list with @uniqueItems",
                    );
                }
                let sparse = shape.traits.contains_key(&smithy("sparse"));
                if sparse {
                    self.require(id, Feature::Sparse, smithy("sparse"));
                }
                let member = self.collection_member(id, "member", shape.member.as_ref(), sparse)?;
                Shape::List(List {
                    internal: shape.traits.contains_key(&smithy("internal")),
                    sensitive: shape.traits.contains_key(&smithy("sensitive")),
                    sparse,
                    member,
                    constraints: constraints(&shape.traits),
                    docs,
                    notes,
                    traits,
                })
            }
            Kind::Map => {
                let sparse = shape.traits.contains_key(&smithy("sparse"));
                if sparse {
                    self.require(id, Feature::Sparse, smithy("sparse"));
                }
                let key = self.collection_member(id, "key", shape.key.as_ref(), false)?;
                let value = self.collection_member(id, "value", shape.value.as_ref(), sparse)?;
                Shape::Map(Map {
                    internal: shape.traits.contains_key(&smithy("internal")),
                    sensitive: shape.traits.contains_key(&smithy("sensitive")),
                    sparse,
                    key,
                    value,
                    constraints: constraints(&shape.traits),
                    docs,
                    notes,
                    traits,
                })
            }
            Kind::Structure if shape.traits.contains_key(&smithy("unitType")) => Shape::Unit {
                internal: shape.traits.contains_key(&smithy("internal")),
            },
            Kind::Structure => Shape::Structure(Structure {
                internal: shape.traits.contains_key(&smithy("internal")),
                sensitive: shape.traits.contains_key(&smithy("sensitive")),
                strict: shape
                    .traits
                    .contains_key(&ShapeId::new("crucible.protocols", "strict")),
                input: shape.traits.contains_key(&smithy("input")),
                members: self.members(id, shape),
                error: self.error_info(id, shape),
                docs,
                notes,
                traits,
            }),
            Kind::Union => Shape::Union(Structure {
                internal: shape.traits.contains_key(&smithy("internal")),
                sensitive: shape.traits.contains_key(&smithy("sensitive")),
                strict: false,
                input: false,
                members: self.members(id, shape),
                error: None,
                docs,
                notes,
                traits,
            }),
            Kind::Service | Kind::Operation | Kind::Resource | Kind::Apply => {
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
        let members: Vec<Member> = shape
            .members
            .iter()
            .map(|(name, member)| self.member(&id.with_member(name), name, member, input))
            .collect();
        members
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
        if self
            .model
            .get(&member.target)
            .is_some_and(|target| target.kind == Kind::Blob)
            && let Some(default) = &default
        {
            use base64::Engine;
            if default.as_str().is_none_or(|text| {
                base64::engine::general_purpose::STANDARD
                    .decode(text)
                    .is_err()
            }) {
                self.diagnostics
                    .push(member_id, "blob default must be valid base64");
            }
        }
        let format = self
            .explicit_timestamp_format(&member.traits)
            .or_else(|| target_traits.and_then(|t| self.explicit_timestamp_format(t)))
            .unwrap_or_else(|| {
                self.protocol.member_timestamp_format(
                    &member.traits,
                    self.http_inputs.contains(&member_id.root()),
                    self.http_outputs.contains(&member_id.root()),
                )
            });
        let wire_default = if self
            .model
            .get(&member.target)
            .is_some_and(|target| target.kind == Kind::Timestamp)
        {
            default.as_ref().and_then(|value| {
                let converted = crate::timestamp_default::wire_value(value, format);
                if converted.is_none() {
                    self.diagnostics.push(
                        member_id,
                        "timestamp default is invalid or outside the supported range",
                    );
                }
                converted
            })
        } else {
            default.clone()
        };
        Member {
            internal: member.traits.contains_key(&smithy("internal")),
            sensitive: member.traits.contains_key(&smithy("sensitive")),
            name: name.to_owned(),
            target: member.target.clone(),
            required: member.traits.contains_key(&smithy("required")),
            default,
            wire_default,
            client_optional: in_input || member.traits.contains_key(&smithy("clientOptional")),
            idempotency_token: member.traits.contains_key(&smithy("idempotencyToken")),
            mutability: self.mutability(member_id, &member.traits),
            constraints: effective,
            timestamp_format: self.explicit_timestamp_format(&member.traits),
            wire_timestamp_format: format,
            docs: string_trait(&member.traits, "documentation"),
            notes: DocNotes::lower(member_id, &member.traits, &mut self.diagnostics),
            traits: member.traits.clone(),
        }
    }

    /// `@aws.cloudformation#cfnMutability`, typed; an unknown value is a
    /// diagnostic rather than a silently mutable property.
    fn mutability(&mut self, member_id: &ShapeId, traits: &Traits) -> Option<Mutability> {
        let value = traits.get(&ShapeId::new("aws.cloudformation", "cfnMutability"))?;
        let parsed = value.as_str().and_then(Mutability::from_cfn);
        if parsed.is_none() {
            self.diagnostics.push(
                member_id,
                "@cfnMutability must be full, create, create-and-read, read or write",
            );
        }
        parsed
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
        let default_status = match fault {
            Fault::Client => 400,
            Fault::Server => 500,
        };
        let status = shape
            .traits
            .get(&smithy("httpError"))
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(default_status);
        let wire_code = id.name().to_owned();
        let retryable = shape
            .traits
            .get(&smithy("retryable"))
            .map(|value| Retryable {
                throttling: value
                    .get("throttling")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        for (name, member) in &shape.members {
            if member.traits.contains_key(&smithy("httpPayload")) {
                self.require(
                    &id.with_member(name),
                    Feature::ErrorPayload,
                    smithy("httpPayload"),
                );
            }
        }
        let bindings = self.response_bindings(id);
        Some(ErrorInfo {
            protocol_headers: self.protocol.response_headers(&bindings, true),
            body_discriminator: Some(wire_code.clone()),
            discriminator_header: self.protocol.error_header().map(str::to_owned),
            timestamp_formats: bindings.timestamp_formats,
            wire_code,
            payload: bindings.payload,
            header_encodings: bindings.header_encodings,
            fault,
            status,
            retryable,
            headers: bindings.headers,
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
