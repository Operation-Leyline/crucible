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
    Binding, Bindings, Bounds, Constraints, Enum, EnumValue, ErrorInfo, Fault, HeaderEncoding,
    IntEnum, IntEnumValue, List, Map, Member, Operation, Paginated, Payload, PayloadKind,
    Retryable, Segment, Service, Shape, Simple, SimpleKind, Structure, TimestampFormat, Uri,
};
use crate::shape_id::ShapeId;
use crate::{Capabilities, Feature, FeatureUse, Protocol};

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
    selected_protocol: Option<Protocol>,
    infrastructure: bool,
) -> Result<Service, Diagnostics> {
    let mut lowerer = Lowerer {
        model: shapes,
        protocol: Protocol::CrucibleJson,
        service_name: service_id.name().to_owned(),
        selected_protocol,
        diagnostics: Diagnostics::default(),
        shapes: BTreeMap::new(),
        visiting: BTreeSet::new(),
        capabilities,
        requirements: BTreeSet::new(),
    };
    let service = lowerer.service(service_id, infrastructure);
    if lowerer.diagnostics.is_empty() {
        Ok(service)
    } else {
        Err(lowerer.diagnostics)
    }
}

pub(crate) struct Lowerer<'m> {
    protocol: Protocol,
    /// The service's shape name, which RPC v2 request paths carry.
    service_name: String,
    selected_protocol: Option<Protocol>,
    pub(crate) model: &'m BTreeMap<ShapeId, ast::Shape>,
    pub(crate) diagnostics: Diagnostics,
    shapes: BTreeMap<ShapeId, Shape>,
    /// Shapes being lowered further up the stack: a recursive reference to
    /// one is complete once the outer call returns.
    visiting: BTreeSet<ShapeId>,
    capabilities: &'m Capabilities,
    requirements: BTreeSet<FeatureUse>,
}

impl<'m> Lowerer<'m> {
    pub(crate) fn reached_ids(&self) -> impl Iterator<Item = ShapeId> + '_ {
        self.shapes.keys().cloned()
    }
    pub(crate) fn protocol_uses_http_bindings(&self) -> bool {
        self.protocol.uses_http_bindings()
    }
    pub(crate) fn protocol(&self) -> Protocol {
        self.protocol
    }
    /// `@timestampFormat`, unless the protocol fixes every timestamp's format.
    fn explicit_timestamp_format(&self, traits: &Traits) -> Option<TimestampFormat> {
        if self.protocol.respects_timestamp_format() {
            timestamp_format(traits)
        } else {
            None
        }
    }
    fn service(&mut self, id: &ShapeId, infrastructure: bool) -> Service {
        let shape = &self.model[id];
        self.check_protocol(id, shape);
        if self.protocol.is_rpc_v2() {
            // RPC v2 errors are identified by their absolute shape ID, so they cannot be renamed.
            for renamed in shape.rename.keys() {
                if self
                    .model
                    .get(renamed)
                    .is_some_and(|target| target.traits.contains_key(&smithy("error")))
                {
                    self.diagnostics.push(
                        id,
                        format!(
                            "@{} services cannot rename error shape {renamed}",
                            self.protocol.trait_id()
                        ),
                    );
                }
            }
        }
        self.check_traits(id, &shape.traits);
        let auth = self.auth(id, shape);
        let auth_schemes = self.auth_configs(id, shape, &auth);
        let endpoint_rules = self.endpoint_rules(id, &shape.traits);
        let default_auth = self.effective_auth(id, &shape.traits, &auth);
        let service_errors: Vec<ShapeId> = shape.errors.iter().map(|r| r.target.clone()).collect();
        let default_pagination = shape.traits.get(&smithy("paginated"));
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
        self.resolve_endpoint_parameters(id, shape, &mut operations, endpoint_rules.as_ref());
        let endpoint_discovery = self.discovery(id, shape, &mut operations);
        let reached: Vec<_> = self.reached_ids().collect();
        let recursive_groups = crate::recursion::lower(self.model, &reached, &mut self.diagnostics);
        let xml_shapes =
            crate::xml::lower(self.model, &reached, self.protocol, &mut self.diagnostics);
        let xml_namespace = if matches!(
            self.protocol,
            Protocol::AwsRestXml | Protocol::AwsQuery | Protocol::Ec2Query
        ) {
            crate::xml::namespace(id, &shape.traits, &mut self.diagnostics)
        } else {
            None
        };
        if self.protocol == Protocol::AwsQuery && xml_namespace.is_none() {
            self.diagnostics
                .push(id, "awsQuery requires service @xmlNamespace");
        }
        let mut waiter_names = BTreeSet::new();
        for operation in &operations {
            for name in operation.waiters.keys() {
                if !waiter_names.insert(name.to_ascii_lowercase()) {
                    self.diagnostics.push(&operation.id, format!("waiter name {name} must be case-insensitively unique across the service"));
                }
            }
        }
        // Client error tables are service-wide and decode by the protocol's wire code.
        // Renaming a shape changes its API name, not that discriminator.
        let mut error_codes = BTreeMap::new();
        for (error_id, error_shape) in &self.shapes {
            if let Shape::Structure(structure) = error_shape
                && let Some(error) = &structure.error
                && let Some(previous) = error_codes.insert(&error.wire_code, error_id)
            {
                self.diagnostics.push(error_id, format!("error wire code {:?} collides with {previous}; client error decoding requires service-wide unique wire codes", error.wire_code));
            }
        }
        self.check_stream_references(&operations);
        let protocol_tests =
            crate::protocol_tests::lower(self.model, &operations, &mut self.diagnostics);
        Service {
            endpoint_discovery,
            recursive_groups,
            xml_namespace,
            xml_shapes,
            protocol: self.protocol,
            protocol_settings: self.protocol_settings(id, shape),
            requirements: std::mem::take(&mut self.requirements).into_iter().collect(),
            id: id.clone(),
            version: shape.version.clone().unwrap_or_default(),
            title: string_trait(&shape.traits, "title"),
            docs: string_trait(&shape.traits, "documentation"),
            auth,
            auth_schemes,
            endpoint_rules,
            protocol_tests,
            operations,
            resources,
            resource_roots,
            resource_references,
            shapes: std::mem::take(&mut self.shapes),
            traits: shape.traits.clone(),
            renames: shape.rename.clone(),
        }
    }

    fn check_protocol(&mut self, id: &ShapeId, shape: &ast::Shape) {
        let declared: Vec<_> = Protocol::ALL
            .iter()
            .copied()
            .filter(|protocol| shape.traits.contains_key(&protocol.trait_id()))
            .collect();
        let protocol = match self.selected_protocol {
            Some(protocol) if declared.contains(&protocol) => protocol,
            Some(protocol) => {
                self.diagnostics.push(
                    id,
                    format!(
                        "selected protocol @{} is not declared by the service",
                        protocol.trait_id()
                    ),
                );
                return;
            }
            None if declared.contains(&Protocol::CrucibleJson) => Protocol::CrucibleJson,
            None => match declared.as_slice() {
                [only] => *only,
                [] => {
                    self.diagnostics.push(id, "the service must use the @crucible.protocols#json protocol or explicitly supported AWS protocol");
                    return;
                }
                _ => {
                    self.diagnostics.push(
                        id,
                        "the service declares several protocols; explicitly select a protocol",
                    );
                    return;
                }
            },
        };
        self.protocol = protocol;
        if let Some(feature) = protocol.feature() {
            self.require(id, feature, protocol.trait_id());
        }
    }

    fn protocol_settings(&mut self, id: &ShapeId, shape: &ast::Shape) -> crate::ProtocolSettings {
        let mut settings = crate::ProtocolSettings::default();
        if matches!(
            self.protocol,
            Protocol::AwsRestJson1
                | Protocol::AwsJson10
                | Protocol::AwsJson11
                | Protocol::SmithyRpcV2Json
        ) {
            settings.omit_null_structure_members = true;
            settings.ignore_unknown_union_type = true;
            settings.non_finite_float_strings = true;
        }
        if self.protocol == Protocol::SmithyRpcV2Json {
            settings.arbitrary_precision_strings = true;
        }
        if self.protocol == Protocol::SmithyRpcV2Cbor {
            // CBOR carries non-finite floats natively, so only null handling applies.
            settings.omit_null_structure_members = true;
            settings.ignore_unknown_union_type = true;
        }
        if matches!(
            self.protocol,
            Protocol::AwsRestXml | Protocol::AwsQuery | Protocol::Ec2Query
        ) {
            // XML text and query values spell non-finite floats NaN, Infinity, -Infinity.
            settings.non_finite_float_strings = true;
        }
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
                "http" | "eventStreamHttp" => {
                    let versions = value.as_array().and_then(|values| {
                        values
                            .iter()
                            .map(|v| v.as_str().filter(|v| !v.is_empty()).map(str::to_owned))
                            .collect::<Option<Vec<_>>>()
                    });
                    if let Some(versions) = versions.filter(|versions| !versions.is_empty()) {
                        if key == "http" {
                            settings.http = versions;
                        } else {
                            settings.event_stream_http = versions;
                        }
                    } else {
                        self.diagnostics.push(id,format!("@{trait_id}.{key} must be a nonempty array of ALPN protocol strings"));
                    }
                }
                "noErrorWrapping" if self.protocol == Protocol::AwsRestXml => {
                    if let Some(enabled) = value.as_bool() {
                        settings.no_error_wrapping = enabled;
                    } else {
                        self.diagnostics.push(id, "noErrorWrapping must be boolean");
                    }
                }
                _ => self
                    .diagnostics
                    .push(id, format!("unsupported @{trait_id} property {key}")),
            }
        }
        if !value.contains_key("eventStreamHttp") {
            settings.event_stream_http = settings.http.clone();
        }
        if settings
            .event_stream_http
            .iter()
            .any(|version| !settings.http.contains(version))
        {
            self.diagnostics
                .push(id, "eventStreamHttp entries must also occur in http");
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
        let auth = if shape.traits.contains_key(&smithy("auth")) {
            self.effective_auth(id, &shape.traits, supported_auth)
        } else {
            default_auth.to_vec()
        };
        let (method, uri, status) = if self.protocol.uses_http_bindings() {
            let Some(http) = shape.traits.get(&smithy("http")) else {
                self.diagnostics.push(id, "operation has no @http trait");
                return None;
            };
            (
                http["method"].as_str().unwrap_or_default().to_owned(),
                self.uri(id, http["uri"].as_str().unwrap_or_default()),
                http.get("code").and_then(Value::as_u64).unwrap_or(200),
            )
        } else if self.protocol.is_rpc_v2() {
            // Shape names, never namespaces or renames, identify the operation.
            let path = format!("/service/{}/operation/{}", self.service_name, id.name());
            ("POST".to_owned(), self.uri(id, &path), 200)
        } else {
            ("POST".to_owned(), self.uri(id, "/"), 200)
        };

        let input = self.io(id, shape.input.as_ref());
        let output = self.io(id, shape.output.as_ref());
        let mut request = input
            .as_ref()
            .map(|input| self.request_bindings(id, input, &uri))
            .unwrap_or_default();
        let mut response = output
            .as_ref()
            .map(|output| self.response_bindings(output))
            .unwrap_or_default();
        if let Some(input) = &input {
            self.event_stream_binding(input, &mut request);
        }
        if let Some(output) = &output {
            self.event_stream_binding(output, &mut response);
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

        let unsigned_payload = auth
            .iter()
            .any(|id| id.namespace() == "aws.auth" && id.name() == "sigv4")
            && shape
                .traits
                .contains_key(&ShapeId::new("aws.auth", "unsignedPayload"));
        let unwrapped_xml_output = shape
            .traits
            .contains_key(&ShapeId::new("aws.customizations", "s3UnwrappedXmlOutput"));
        if unwrapped_xml_output
            && (self.protocol != Protocol::AwsRestXml
                || response.body.len() != 1
                || output
                    .as_ref()
                    .is_none_or(|output| self.model[output].members.len() != 1))
        {
            self.diagnostics.push(
                id,
                "@s3UnwrappedXmlOutput requires restXml and an output with exactly one body member",
            );
        }
        Some(Operation {
            unwrapped_xml_output,
            unsigned_payload,
            discovered_endpoint: None,
            checksum: self.checksum(id, shape, input.as_ref()),
            id: id.clone(),
            auth,
            optional_auth: shape.traits.contains_key(&smithy("optionalAuth")),
            endpoint: self.endpoint_host_prefix(id, shape, input.as_ref()),
            endpoint_parameters: BTreeMap::new(),
            waiters: self.waiters(id, &shape.traits, output.is_some()),
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
        if !self.protocol.uses_http_bindings() {
            return Bindings {
                body: self.model[input].members.keys().cloned().collect(),
                ..Bindings::default()
            };
        }
        let mut bindings = Bindings::default();
        for (name, member) in &self.model[input].members {
            let member_id = input.with_member(name);
            if member.traits.contains_key(&smithy("httpResponseCode")) {
                self.diagnostics
                    .push(&member_id, "@httpResponseCode cannot bind an input member");
            }
            if member.traits.contains_key(&smithy("httpPayload")) {
                bindings.payload = self.payload(&member_id, member);
            } else if member.traits.contains_key(&smithy("httpLabel")) {
                if !member.traits.contains_key(&smithy("required")) {
                    self.diagnostics
                        .push(&member_id, "an @httpLabel member must be @required");
                }
                self.check_scalar(&member_id, &member.target, "@httpLabel", false);
                if uri.greedy_labels.contains(name)
                    && !self
                        .model
                        .get(&member.target)
                        .is_some_and(|s| matches!(s.kind, Kind::String | Kind::Enum))
                {
                    self.diagnostics
                        .push(&member_id, "a greedy URI label must target a string");
                }
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
            } else if member.traits.contains_key(&smithy("httpQueryParams")) {
                self.require(&member_id, Feature::QueryParams, smithy("httpQueryParams"));
                self.check_map_binding(&member_id, member, true);
                if bindings.query_params.replace(name.clone()).is_some() {
                    self.diagnostics
                        .push(input, "only one member may bind @httpQueryParams");
                }
            } else if let Some(prefix) = string_trait(&member.traits, "httpPrefixHeaders") {
                self.require(
                    &member_id,
                    Feature::PrefixHeaders,
                    smithy("httpPrefixHeaders"),
                );
                self.check_map_binding(&member_id, member, false);
                bindings.prefix_headers.push(Binding {
                    member: name.clone(),
                    name: prefix,
                });
                if bindings.prefix_headers.len() > 1 {
                    self.diagnostics
                        .push(input, "only one member may bind @httpPrefixHeaders");
                }
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
        self.check_prefix_headers(input, &bindings);
        self.header_encodings(input, &mut bindings);
        self.binding_timestamp_formats(input, &mut bindings);
        bindings
    }

    fn response_bindings(&mut self, output: &ShapeId) -> Bindings {
        if !self.protocol.uses_http_bindings() {
            return Bindings {
                body: self.model[output].members.keys().cloned().collect(),
                ..Bindings::default()
            };
        }
        let mut bindings = Bindings::default();
        for (name, member) in &self.model[output].members {
            if let Some(prefix) = string_trait(&member.traits, "httpPrefixHeaders") {
                self.require(
                    &output.with_member(name),
                    Feature::PrefixHeaders,
                    smithy("httpPrefixHeaders"),
                );
                self.check_map_binding(&output.with_member(name), member, false);
                bindings.prefix_headers.push(Binding {
                    member: name.clone(),
                    name: prefix,
                });
                if bindings.prefix_headers.len() > 1 {
                    self.diagnostics
                        .push(output, "only one member may bind @httpPrefixHeaders");
                }
                continue;
            }
            if member.traits.contains_key(&smithy("httpResponseCode")) {
                self.require(
                    &output.with_member(name),
                    Feature::ResponseCode,
                    smithy("httpResponseCode"),
                );
                if !self
                    .model
                    .get(&member.target)
                    .is_some_and(|s| s.kind == Kind::Integer)
                {
                    self.diagnostics.push(
                        &output.with_member(name),
                        "@httpResponseCode must target integer",
                    );
                }
                if bindings.response_code.replace(name.clone()).is_some() {
                    self.diagnostics
                        .push(output, "only one member may bind @httpResponseCode");
                }
                continue;
            }
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
        self.check_prefix_headers(output, &bindings);
        self.header_encodings(output, &mut bindings);
        self.binding_timestamp_formats(output, &mut bindings);
        bindings
    }

    /// Resolve the representation and content type of the complete HTTP body.
    fn payload(&mut self, member_id: &ShapeId, member: &ast::Member) -> Option<Payload> {
        let target = self.model.get(&member.target)?;
        let kind = match target.kind {
            Kind::Union if target.traits.contains_key(&smithy("streaming")) => {
                PayloadKind::EventStream
            }
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
        if !matches!(kind, PayloadKind::Blob | PayloadKind::EventStream) {
            self.require(member_id, Feature::NonBlobPayload, smithy("httpPayload"));
        }
        let media_type = string_trait(&target.traits, "mediaType").unwrap_or_else(|| {
            match kind {
                PayloadKind::Blob => "application/octet-stream",
                PayloadKind::String => "text/plain",
                PayloadKind::EventStream => "application/vnd.amazon.eventstream",
                _ if self.protocol == Protocol::AwsRestXml => "application/xml",
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
            streaming: target.traits.contains_key(&smithy("streaming")),
            requires_length: target.traits.contains_key(&smithy("requiresLength")),
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

    fn check_prefix_headers(&mut self, structure: &ShapeId, bindings: &Bindings) {
        for prefix in &bindings.prefix_headers {
            if prefix.name.is_empty() {
                continue;
            }
            for header in &bindings.headers {
                if header
                    .name
                    .to_ascii_lowercase()
                    .starts_with(&prefix.name.to_ascii_lowercase())
                {
                    self.diagnostics.push(
                        &structure.with_member(&prefix.member),
                        format!(
                            "@httpPrefixHeaders prefix {:?} overlaps explicit header {:?}",
                            prefix.name, header.name
                        ),
                    );
                }
            }
        }
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
            let binding = explicit.unwrap_or_else(|| {
                if header {
                    self.protocol.timestamp_format(true)
                } else {
                    TimestampFormat::DateTime
                }
            });
            if binding != document {
                bindings.timestamp_formats.insert(name.clone(), binding);
            }
        }
    }

    fn header_encodings(&mut self, structure: &ShapeId, bindings: &mut Bindings) {
        for binding in bindings.headers.iter().chain(&bindings.prefix_headers) {
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

    fn check_scalar(&mut self, member: &ShapeId, target: &ShapeId, binding: &str, lists: bool) {
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
        if binding == "@httpHeader"
            && header_target.is_some_and(|s| {
                matches!(s.kind, Kind::String | Kind::Enum)
                    && s.traits.contains_key(&smithy("mediaType"))
            })
        {
            self.require(member, Feature::MediaTypeHeaders, smithy("mediaType"));
        }
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

    /// XML has no null element, so a sparse entry could not round-trip.
    fn check_xml_sparse(&mut self, id: &ShapeId, sparse: bool) {
        if sparse
            && matches!(
                self.protocol,
                Protocol::AwsRestXml | Protocol::AwsQuery | Protocol::Ec2Query
            )
        {
            self.diagnostics.push(
                id,
                "@sparse collections are not supported by the selected XML protocol",
            );
        }
    }

    fn check_traits(&mut self, id: &ShapeId, traits: &Traits) {
        for (name, feature) in [
            ("hostLabel", Feature::EndpointHostPrefix),
            ("optionalAuth", Feature::OptionalAuth),
            ("sparse", Feature::Sparse),
        ] {
            if traits.contains_key(&smithy(name)) {
                self.require(id, feature, smithy(name));
            }
        }
        if self.protocol.uses_http_bindings()
            && traits
                .get(&smithy("httpPrefixHeaders"))
                .is_some_and(|v| !v.is_string())
        {
            self.diagnostics
                .push(id, "@httpPrefixHeaders must be a string");
        }
        if traits.contains_key(&smithy("requestCompression")) {
            self.diagnostics
                .push(id, "@requestCompression is not supported");
        }
        if self.protocol == Protocol::AwsJson10
            && traits.contains_key(&ShapeId::new("aws.protocols", "awsQueryCompatible"))
        {
            self.diagnostics
                .push(id, "@aws.protocols#awsQueryCompatible is not supported");
        }
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

    fn check_map_binding(&mut self, id: &ShapeId, member: &ast::Member, lists: bool) {
        let Some(target) = self.model.get(&member.target) else {
            return;
        };
        let value = target
            .value
            .as_ref()
            .and_then(|value| self.model.get(&value.target));
        let valid_value = value.is_some_and(|value| {
            matches!(value.kind, Kind::String | Kind::Enum)
                || lists
                    && value.kind == Kind::List
                    && value
                        .member
                        .as_ref()
                        .and_then(|m| self.model.get(&m.target))
                        .is_some_and(|s| matches!(s.kind, Kind::String | Kind::Enum))
        });
        if target.kind != Kind::Map
            || !valid_value
            || !lists && target.traits.contains_key(&smithy("sparse"))
        {
            self.diagnostics.push(
                id,
                if lists {
                    "@httpQueryParams requires a map of strings or string lists"
                } else {
                    "@httpPrefixHeaders requires a dense map of strings"
                },
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
        self.check_stream_shape(id, shape);
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
        let explicit_format = self.explicit_timestamp_format(&shape.traits);
        let simple = |kind| {
            Shape::Simple(Simple {
                kind,
                streaming: shape.traits.contains_key(&smithy("streaming")),
                requires_length: shape.traits.contains_key(&smithy("requiresLength")),
                constraints: constraints(&shape.traits),
                timestamp_format: explicit_format,
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
            Kind::BigInteger => {
                self.require(id, Feature::BigInteger, smithy("BigInteger"));
                simple(SimpleKind::BigInteger)
            }
            Kind::BigDecimal => {
                self.require(id, Feature::BigDecimal, smithy("BigDecimal"));
                simple(SimpleKind::BigDecimal)
            }
            Kind::Timestamp => simple(SimpleKind::Timestamp),
            Kind::Document => {
                if matches!(
                    self.protocol,
                    Protocol::AwsRestXml | Protocol::AwsQuery | Protocol::Ec2Query
                ) {
                    self.diagnostics.push(
                        id,
                        "document shapes are not supported by the selected XML protocol",
                    );
                }
                if self.protocol == Protocol::SmithyRpcV2Cbor {
                    self.diagnostics.push(
                        id,
                        "document shapes are not supported by smithy.protocols#rpcv2Cbor",
                    );
                }
                simple(SimpleKind::Document)
            }
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
                        Some(value)=>{
                            if !numbers.insert(value) {self.diagnostics.push(&id.with_member(name),"intEnum values must be unique");}
                            values.push(IntEnumValue {name:name.clone(),value,docs:string_trait(&member.traits,"documentation")});
                        }
                        None=>self.diagnostics.push(&id.with_member(name),"intEnum member must have an integer @enumValue within signed 32-bit range"),
                    }
                }
                Shape::IntEnum(IntEnum {
                    values,
                    docs,
                    traits,
                })
            }
            Kind::List | Kind::Set => {
                let sparse = shape.traits.contains_key(&smithy("sparse"));
                let is_set = shape.kind == Kind::Set;
                if is_set {
                    self.require(id, Feature::Sets, smithy("Set"));
                }
                if is_set && sparse {
                    self.diagnostics.push(id, "a set cannot be sparse");
                }
                self.check_xml_sparse(id, sparse);
                let member = self.collection_member(id, "member", shape.member.as_ref(), sparse)?;
                let mut constraints = constraints(&shape.traits);
                constraints.unique_items |= is_set;
                Shape::List(List {
                    sparse,
                    is_set,
                    member,
                    constraints,
                    docs,
                    traits,
                })
            }
            Kind::Map => {
                let sparse = shape.traits.contains_key(&smithy("sparse"));
                self.check_xml_sparse(id, sparse);
                let key = self.collection_member(id, "key", shape.key.as_ref(), false)?;
                let value = self.collection_member(id, "value", shape.value.as_ref(), sparse)?;
                Shape::Map(Map {
                    sparse,
                    key,
                    value,
                    constraints: constraints(&shape.traits),
                    docs,
                    traits,
                })
            }
            Kind::Structure if shape.traits.contains_key(&smithy("unitType")) => Shape::Unit,
            Kind::Structure => Shape::Structure(Structure {
                streaming: false,
                members: self.members(id, shape),
                error: self.error_info(id, shape),
                docs,
                traits,
            }),
            Kind::Union => Shape::Union(Structure {
                streaming: shape.traits.contains_key(&smithy("streaming")),
                members: self.members(id, shape),
                error: None,
                docs,
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
        let mut names = BTreeSet::new();
        for member in &members {
            if !names.insert(&member.wire_name) {
                self.diagnostics.push(
                    &id.with_member(&member.name),
                    format!("duplicate JSON member name {:?}", member.wire_name),
                );
            }
        }
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
        if self.protocol.uses_json_names()
            && member
                .traits
                .get(&smithy("jsonName"))
                .is_some_and(|value| !value.is_string())
        {
            self.diagnostics
                .push(member_id, "@jsonName must be a string");
        }
        if self.protocol.uses_json_names() && member.traits.contains_key(&smithy("jsonName")) {
            self.require(member_id, Feature::JsonName, smithy("jsonName"));
        }
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
            .unwrap_or_else(|| self.protocol.member_timestamp_format(&member.traits));
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
            event_binding: self.event_binding(member_id, member),
            name: name.to_owned(),
            wire_name: if self.protocol.uses_json_names() {
                string_trait(&member.traits, "jsonName").unwrap_or_else(|| name.to_owned())
            } else {
                name.to_owned()
            },
            target: member.target.clone(),
            required: member.traits.contains_key(&smithy("required")),
            default,
            wire_default,
            client_optional: in_input || member.traits.contains_key(&smithy("clientOptional")),
            constraints: effective,
            timestamp_format: self.explicit_timestamp_format(&member.traits),
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
        let default_status = match fault {
            Fault::Client => 400,
            Fault::Server => 500,
        };
        let query_error = if self.protocol == Protocol::AwsQuery {
            shape
                .traits
                .get(&ShapeId::new("aws.protocols", "awsQueryError"))
        } else {
            None
        };
        let status = if let Some(value) = query_error {
            value.get("httpResponseCode").and_then(Value::as_u64)
        } else if self.protocol.uses_http_bindings() || self.protocol.is_rpc_v2() {
            shape
                .traits
                .get(&smithy("httpError"))
                .and_then(Value::as_u64)
        } else {
            None
        }
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(default_status);
        let wire_code = query_error
            .and_then(|value| value.get("code"))
            .and_then(Value::as_str)
            .unwrap_or(id.name())
            .to_owned();
        if query_error.is_some_and(|value| {
            value.get("code").and_then(Value::as_str).is_none()
                || value
                    .get("httpResponseCode")
                    .and_then(Value::as_u64)
                    .is_none_or(|code| !(100..=599).contains(&code))
        }) {
            self.diagnostics.push(
                id,
                "@awsQueryError requires code:string and httpResponseCode:HTTP status",
            );
        }
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
            if self.protocol.uses_http_bindings()
                && member.traits.contains_key(&smithy("httpPayload"))
            {
                self.require(
                    &id.with_member(name),
                    Feature::ErrorPayload,
                    smithy("httpPayload"),
                );
                if self.protocol == Protocol::AwsRestXml {
                    self.diagnostics.push(
                        &id.with_member(name),
                        "raw error payloads are not supported by the selected XML error envelope",
                    );
                }
            }
        }
        let bindings = self.response_bindings(id);
        Some(ErrorInfo {
            timestamp_formats: bindings.timestamp_formats,
            wire_code,
            payload: bindings.payload,
            header_encodings: bindings.header_encodings,
            fault,
            status,
            retryable,
            headers: bindings.headers,
            prefix_headers: bindings.prefix_headers,
            response_code: bindings.response_code,
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
