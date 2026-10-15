//! The top level of the generated file: per operation a marker type, its
//! `OperationInfo`, and the trait the service implements; for the service its
//! `ServiceInfo` and the registration functions.

use crucible_smithy::ir::{DocNotes, Operation, Segment, Service, Shape};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::errors::{error_shapes, info_static};
use crate::info::{constant, custom_traits, members};
use crate::names::{documented, function, literal};

pub(crate) struct Functions {
    pub classify: String,
    pub reject: Option<String>,
    pub internal: Option<String>,
    pub unknown: Option<String>,
}

pub(crate) fn emit(
    service: &Service,
    functions: Functions,
    mcp: bool,
    diagnostics: &mut Diagnostics,
) -> String {
    let mut out = String::new();
    for operation in &service.operations {
        out.push_str(&operation_items(service, operation, diagnostics));
        if function(operation.name()) == "build" {
            diagnostics.push(
                &operation.id,
                "an operation named Build collides with OperationsBuilder::build",
            );
        }
    }
    out.push('\n');
    out.push_str(&functions.classify);
    if let Some(reject) = &functions.reject {
        out.push('\n');
        out.push_str(reject);
    }
    if let Some(internal) = &functions.internal {
        out.push('\n');
        out.push_str(internal);
    }
    if let Some(unknown) = &functions.unknown {
        out.push('\n');
        out.push_str(unknown);
    }
    out.push_str(&service_info(service, &functions, mcp));
    out.push_str(&registration(service));
    out
}

fn operation_static(operation: &Operation) -> String {
    format!("{}_OPERATION", constant(operation.name()))
}

pub(crate) fn input_type(service: &Service, operation: &Operation) -> String {
    operation.input.as_ref().map_or(
        "::crucible_server_runtime_rust::__generated::operation::Unit".to_owned(),
        |id| format!("model::{}", service.name_of(id)),
    )
}

pub(crate) fn output_type(service: &Service, operation: &Operation) -> String {
    operation.output.as_ref().map_or("()".to_owned(), |id| {
        format!("model::{}", service.name_of(id))
    })
}

pub(crate) fn structure<'s>(
    service: &'s Service,
    id: Option<&ShapeId>,
) -> Option<&'s crucible_smithy::ir::Structure> {
    match service.shape(id?)? {
        Shape::Structure(structure) => Some(structure),
        _ => None,
    }
}

fn operation_items(service: &Service, op: &Operation, diagnostics: &mut Diagnostics) -> String {
    let name = op.name();
    // REQUIREMENTS and CHECKS go in the marker's impl; obligations beside it.
    let permissions = crate::permissions::marker_items(service, op, diagnostics);
    let mut out = marker(service, op, &output_type(service, op), &permissions);
    out.push_str(&crate::permissions::obligations(service, op));
    // The marker type is what callers name, so it carries `#[deprecated]`. The
    // trait is what a service implements: warning there would only nag the
    // author about an operation they are still required to serve.
    let served = DocNotes {
        deprecated: None,
        ..op.notes.clone()
    };
    documented(&mut out, "", op.docs.as_deref(), &served);
    let parameter = if op.input.is_some() {
        format!(
            ", input: {}",
            input_type(service, op).trim_start_matches("model::")
        )
    } else {
        String::new()
    };
    out.push_str(&format!(
        "pub trait {name}Operation: ::crucible_server_runtime_rust::__generated::WithContext + Send + Sync + 'static {{\n    fn {}(&self{parameter}) -> impl ::std::future::Future<Output = ::crucible_server_runtime_rust::__generated::Result<{}>> + Send;\n}}\n",
        function(name),
        output_type(service, op).trim_start_matches("model::"),
    ));
    out
}

/// The server's marker: [`marker_for`] with its permission items.
fn marker(service: &Service, op: &Operation, output_ty: &str, permissions: &str) -> String {
    marker_for(service, op, output_ty, false, permissions)
}

/// The marker type, its `OperationInfo` and its `Operation` impl, with
/// `output_ty` as the output type: a client reads a missing output as `Unit`.
/// `server_items` are the server's own associated items (requirements).
pub(crate) fn marker_for(
    service: &Service,
    op: &Operation,
    output_ty: &str,
    client: bool,
    server_items: &str,
) -> String {
    let name = op.name();
    let mut out = String::from("\n");
    documented(&mut out, "", op.docs.as_deref(), &op.notes);
    out.push_str(&format!("pub struct {name};\n\n"));

    let path: Vec<String> = op
        .uri
        .segments
        .iter()
        .map(|segment| match segment {
            Segment::Literal(text) => {
                format!(
                    "::crucible_server_runtime_rust::__generated::operation::Segment::Literal({})",
                    literal(text)
                )
            }
            Segment::Label(label) => {
                let kind = if op.uri.greedy_labels.contains(label) {
                    "GreedyLabel"
                } else {
                    "Label"
                };
                let wire = structure(service, op.input.as_ref())
                    .and_then(|s| s.members.iter().find(|m| &m.name == label))
                    .map_or(label.as_str(), |m| m.wire_name.as_str());
                format!(
                    "::crucible_server_runtime_rust::__generated::operation::Segment::{kind}({})",
                    literal(wire)
                )
            }
        })
        .collect();
    let query = op
        .uri
        .query
        .iter()
        .map(|(name, value)| {
            let value = value.as_ref().map_or("None".to_owned(), |value| {
                format!("Some({})", literal(value))
            });
            format!("({}, {value})", literal(name))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let input = structure(service, op.input.as_ref())
        .map_or("&[]".to_owned(), |s| members(service, s, &op.request));
    let output = structure(service, op.output.as_ref())
        .map_or("&[]".to_owned(), |s| members(service, s, &op.response));
    let errors: Vec<String> = op
        .errors
        .iter()
        .map(|id| {
            let Shape::Structure(shape) = service.shape(id).expect("lowered error") else {
                unreachable!("error is a structure");
            };
            literal(
                &shape
                    .error
                    .as_ref()
                    .expect("lowered error metadata")
                    .wire_code,
            )
        })
        .collect();
    let resource = op.resource.as_ref().map_or("None".to_owned(), |id| {
        format!("Some({})", literal(service.name_of(id)))
    });
    let token = op
        .idempotency_token
        .as_ref()
        .map_or("None".to_owned(), |token| {
            format!("Some({})", literal(token))
        });
    out.push_str(&format!(
        "static {}: ::crucible_server_runtime_rust::__generated::OperationInfo = ::crucible_server_runtime_rust::__generated::operation_info(\n    {},\n    {},\n    {resource},\n    {},\n    &[{}],\n    &[{query}],\n    {},\n    {input},\n    {output},\n    &[{}],\n    {},\n    {},\n    {},\n    {token},\n    {},\n);\n\n",
        operation_static(op),
        literal(&op.id.to_string()),
        literal(name),
        literal(&op.method),
        path.join(", "),
        op.status,
        errors.join(", "),
        op.readonly,
        op.idempotent,
        op.internal,
        custom_traits(&op.traits),
    ));
    let mut streaming = String::new();
    if let Some(member) = op
        .response
        .response_code
        .as_ref()
        .and_then(|name| structure(service, op.output.as_ref()).and_then(|s| s.member(name)))
    {
        let rust = crate::names::field(&member.name);
        let get = if client && member.optional_on_client() {
            format!(
                "value.{rust}.value().map(|value| ::crucible_server_runtime_rust::__generated::operation::response_status(i64::from(*value)))"
            )
        } else if !client && member.optional_on_server() {
            format!(
                "value.{rust}.as_ref().map(|value| ::crucible_server_runtime_rust::__generated::operation::response_status(i64::from(*value)))"
            )
        } else {
            format!(
                "Some(::crucible_server_runtime_rust::__generated::operation::response_status(i64::from(value.{rust})))"
            )
        };
        streaming.push_str(&format!(
            "    fn output_status(value: &Self::Output) -> ::std::option::Option<u16> {{ {get} }}\n"
        ));
    }
    for (direction, id, bindings) in [
        ("input", op.input.as_ref(), &op.request),
        ("output", op.output.as_ref(), &op.response),
    ] {
        if let Some(payload) = bindings
            .payload
            .as_ref()
            .filter(|p| p.streaming && p.kind != crucible_smithy::ir::PayloadKind::EventStream)
        {
            let member = structure(service, id)
                .and_then(|s| s.member(&payload.member))
                .expect("lowered payload");
            let rust = crate::names::field(&member.name);
            let ty = if direction == "input" {
                "Input"
            } else {
                "Output"
            };
            let (get, set) = if client && member.optional_on_client() {
                (
                    format!("value.{rust}.value().cloned()"),
                    "::crucible_server_runtime_rust::__generated::Field::Value(stream)",
                )
            } else if !client && member.optional_on_server() {
                (format!("value.{rust}.clone()"), "Some(stream)")
            } else {
                (format!("Some(value.{rust}.clone())"), "stream")
            };
            streaming.push_str(&format!("    fn {direction}_stream(value: &Self::{ty}) -> ::std::option::Option<::crucible_server_runtime_rust::__generated::ByteStream> {{ {get} }}\n    fn set_{direction}_stream(value: &mut Self::{ty}, stream: ::crucible_server_runtime_rust::__generated::ByteStream) {{ value.{rust} = {set}; }}\n"));
        }
    }
    let auth = op
        .auth
        .iter()
        .filter_map(|id| service.auth_schemes.get(id))
        .map(|scheme| {
            use crucible_smithy::ir::{ApiKeyLocation, AuthScheme};
            let value = match scheme {
                AuthScheme::Bearer => "Bearer".to_owned(),
                AuthScheme::Basic => "Basic".to_owned(),
                AuthScheme::ApiKey {
                    name,
                    location,
                    scheme,
                } => format!(
                    "ApiKey {{ name: {}, query: {}, scheme: {} }}",
                    literal(name),
                    *location == ApiKeyLocation::Query,
                    scheme
                        .as_deref()
                        .map_or("None".to_owned(), |scheme| format!(
                            "Some({})",
                            literal(scheme)
                        ))
                ),
                AuthScheme::SigV4 { signing_name } => {
                    format!("SigV4 {{ signing_name: {} }}", literal(signing_name))
                }
                // Rust does not declare these capabilities, so emit rejects them first.
                AuthScheme::Digest | AuthScheme::SigV4a { .. } => {
                    unreachable!("undeclared Rust auth capability")
                }
            };
            format!("::crucible_server_runtime_rust::__generated::operation::AuthScheme::{value}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut auth = format!(
        "    const AUTH_SCHEMES: ::std::option::Option<&'static [::crucible_server_runtime_rust::__generated::operation::AuthScheme]> = Some(&[{auth}]);\n    const OPTIONAL_AUTH: bool = {};\n",
        op.optional_auth
    );
    if !client {
        auth.push_str(&scope_items(service, op));
        auth.push_str(&pagination_items(service, op));
        auth.push_str(&idempotency_items(service, op, output_ty));
        auth.push_str(server_items);
    }
    auth.push_str(&wire(service, op));
    auth.push_str(&crate::events::marker_items(service, op, client));
    if op.input.is_none() {
        auth.push_str("    const HAS_INPUT: bool = false;\n");
    }
    if op.output.is_none() {
        auth.push_str("    const HAS_OUTPUT: bool = false;\n");
    }
    if op.unsigned_payload {
        auth.push_str("    const UNSIGNED_PAYLOAD: bool = true;\n");
    }
    // The marker is `#[deprecated]` for callers; the framework wiring below is
    // generated code that must name it regardless.
    let allow = if op.notes.deprecated.is_some() {
        "#[allow(deprecated)]\n"
    } else {
        ""
    };
    out.push_str(&format!(
        "{allow}impl ::crucible_server_runtime_rust::__generated::Operation for {name} {{\n    const AUTH_BEARER: bool = {};\n{auth}    const INFO: &'static ::crucible_server_runtime_rust::__generated::OperationInfo = &{};\n    type Input = {};\n    type Output = {};\n{streaming}}}\n\n",
        op.auth.iter().any(|id| service.is_bearer(id)),
        operation_static(op),
        input_type(service, op),
        output_ty,
    ));
    out
}

/// A marker's protocol and checksum description, when they differ
/// from the defaults: `crucible.protocols#json` and no checksum.
fn wire(service: &Service, op: &Operation) -> String {
    use crucible_smithy::ir::{Checksum, ChecksumAlgorithm};
    let mut out = String::new();
    // The release a generated client names in `Crucible-Version`.
    if let Some(version) = &service.api_version {
        out.push_str(&format!(
            "    const API_VERSION: ::std::option::Option<&'static str> = ::std::option::Option::Some({});\n",
            literal(version)
        ));
    }
    let wire = |name: &Option<String>| {
        name.as_ref().map_or("None".to_owned(), |name| {
            let wire = structure(service, op.input.as_ref())
                .and_then(|s| s.member(name))
                .map_or(name.as_str(), |m| m.wire_name.as_str());
            format!("Some({})", literal(wire))
        })
    };
    let checksum = match &op.checksum {
        None => return out,
        Some(Checksum::Md5) => "Md5".to_owned(),
        Some(Checksum::AwsFlexible {
            request_required,
            request_algorithm_member,
            validation_mode_member,
            response_algorithms,
            ..
        }) => {
            let algorithms = response_algorithms
                .iter()
                .map(|algorithm| {
                    let name = match algorithm {
                        ChecksumAlgorithm::Crc32 => "Crc32",
                        ChecksumAlgorithm::Crc32c => "Crc32c",
                        ChecksumAlgorithm::Crc64nvme => "Crc64Nvme",
                        ChecksumAlgorithm::Md5 => "Md5",
                        ChecksumAlgorithm::Sha1 => "Sha1",
                        ChecksumAlgorithm::Sha256 => "Sha256",
                        ChecksumAlgorithm::Sha512 => "Sha512",
                        ChecksumAlgorithm::Xxhash64 => "Xxhash64",
                        ChecksumAlgorithm::Xxhash3 => "Xxhash3",
                        ChecksumAlgorithm::Xxhash128 => "Xxhash128",
                    };
                    format!("::crucible_server_runtime_rust::__generated::operation::ChecksumAlgorithm::{name}")
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Flexible {{ request_required: {request_required}, request_algorithm_member: {}, validation_mode_member: {}, response_algorithms: &[{algorithms}] }}",
                wire(request_algorithm_member),
                wire(validation_mode_member)
            )
        }
    };
    out.push_str(&format!(
        "    const CHECKSUM: ::std::option::Option<::crucible_server_runtime_rust::__generated::operation::Checksum> = Some(::crucible_server_runtime_rust::__generated::operation::Checksum::{checksum});\n"
    ));
    out
}

fn service_info(service: &Service, functions: &Functions, mcp: bool) -> String {
    let cors = service.cors.as_ref().map_or_else(|| "None".to_owned(), |cors| {
        // Older versions' requests carry their own headers and methods, and
        // a browser client names its version and reads the echoed one.
        let merged = |pick: fn(&crucible_smithy::ir::Cors) -> &Vec<String>, extra: &[&str]| {
            let mut values: std::collections::BTreeSet<String> = pick(cors).iter().cloned().collect();
            for version in &service.versions {
                if let Some(older) = &version.service.cors {
                    values.extend(pick(older).iter().cloned());
                }
            }
            // Generated clients of a `.cru` service always send their release,
            // from the first, before any migration exists.
            if service.api_version.is_some() || !service.versions.is_empty() {
                values.extend(extra.iter().map(|value| (*value).to_owned()));
            }
            values.iter().map(|value| literal(value)).collect::<Vec<_>>().join(", ")
        };
        format!(
            "Some(&::crucible_server_runtime_rust::__generated::operation::CorsInfo {{ origin: {}, max_age: {}, allow_credentials: {}, allowed_headers: &[{}], allowed_header_prefixes: &[{}], exposed_headers: &[{}], methods: &[{}] }})",
            literal(&cors.origin),
            cors.max_age,
            cors.allow_credentials,
            merged(|cors| &cors.allowed_headers, &["crucible-version"]),
            merged(|cors| &cors.allowed_header_prefixes, &[]),
            merged(|cors| &cors.exposed_headers, &["crucible-version", "sunset"]),
            merged(|cors| &cors.methods, &[]),
        )
    });
    let operations: Vec<String> = service
        .operations
        .iter()
        .map(|op| format!("&{}", operation_static(op)))
        .collect();
    let errors: Vec<String> = error_shapes(service)
        .into_iter()
        .map(|(id, _)| format!("&errors::{}", info_static(service.name_of(id))))
        .collect();
    let option = |present: bool, name: &str| {
        if present {
            format!("Some({name})")
        } else {
            "None".to_owned()
        }
    };
    let mcp_tools = if mcp { "Some(MCP_TOOLS)" } else { "None" };
    format!(
        "\n/// The service, as the framework sees it.\npub static SERVICE: ::crucible_server_runtime_rust::__generated::ServiceInfo = ::crucible_server_runtime_rust::__generated::service_info(\n    {},\n    {},\n    {cors},\n    {mcp_tools},\n    &[\n        {}{}\n    ],\n    &[{}],\n    classify,\n    {},\n    {},\n    {},\n){}{};\n",
        literal(&service.id.to_string()),
        literal(&service.version),
        operations.join(",\n        "),
        if operations.is_empty() { "" } else { "," },
        errors.join(", "),
        option(functions.reject.is_some(), "reject"),
        option(functions.internal.is_some(), "internal"),
        option(functions.unknown.is_some(), "unknown"),
        crate::permissions::registry(service),
        crate::permissions::not_found(service),
    )
}

fn registration(service: &Service) -> String {
    let mut out = String::from(
        "\n/// Registers implementations one operation at a time, for a service split\n/// across several types. `build` names any operation left out.\npub struct OperationsBuilder(::crucible_server_runtime_rust::__generated::OperationsBuilder);\n\nimpl OperationsBuilder {\n",
    );
    for op in &service.operations {
        let method = function(op.name());
        let call = if op.input.is_some() {
            format!("|implementation, input| async move {{ implementation.{method}(input).await }}")
        } else {
            format!("|implementation, _| async move {{ implementation.{method}().await }}")
        };
        out.push_str(&format!(
            "    /// Serves {name} with `implementation`.\n{allow}    pub fn {method}<S: {name}Operation>(self, implementation: ::std::sync::Arc<S>) -> Self {{\n        Self(self.0.add::<{name}, S, _>(implementation, {call}))\n    }}\n\n",
            name = op.name(),
            allow = if op.notes.deprecated.is_some() {
                "    #[allow(deprecated)]\n"
            } else {
                ""
            },
        ));
    }
    out.push_str(
        "    pub fn build(self) -> ::std::result::Result<::crucible_server_runtime_rust::__generated::Operations, ::crucible_server_runtime_rust::__generated::MissingOperations> {\n        self.0.build()\n    }\n}\n\n",
    );
    if let (true, Some(release)) = (service.versions.is_empty(), &service.api_version) {
        // One release: its clients name it, and any other is refused.
        out.push_str(&format!(
            "pub fn builder() -> OperationsBuilder {{\n    OperationsBuilder(::crucible_server_runtime_rust::__generated::Operations::builder(&SERVICE).versions(::crucible_server_runtime_rust::__generated::versioning::ApiVersion::new(0, {}), &[]))\n}}\n\n",
            literal(release)
        ));
    } else if service.versions.is_empty() {
        out.push_str("pub fn builder() -> OperationsBuilder {\n    OperationsBuilder(::crucible_server_runtime_rust::__generated::Operations::builder(&SERVICE))\n}\n\n");
    } else {
        // The older versions are served by the same implementations.
        out.push_str("pub fn builder() -> OperationsBuilder {\n    OperationsBuilder(versions::register(::crucible_server_runtime_rust::__generated::Operations::builder(&SERVICE)))\n}\n\n");
    }
    let mut bounds: Vec<String> = service
        .operations
        .iter()
        .map(|op| format!("{}Operation", op.name()))
        .collect();
    let mut calls: String = service
        .operations
        .iter()
        .map(|op| format!("\n        .{}(implementation.clone())", function(op.name())))
        .collect();
    // Older versions' hooks, for a model with migrations that need them.
    if !service.versions.is_empty() {
        bounds.push("versions::Migrations".to_owned());
        calls.push_str("\n        .migrations(implementation.clone())");
    }
    let implementation = if service.operations.is_empty() && service.versions.is_empty() {
        "_implementation"
    } else {
        "implementation"
    };
    out.push_str(&format!(
        "/// Every operation, served by one type: checked at compile time.\npub fn operations<T>({implementation}: ::std::sync::Arc<T>) -> ::crucible_server_runtime_rust::__generated::Operations\nwhere\n    T: {},\n{{\n    builder(){calls}\n        .build()\n        .expect(\"every operation is registered\")\n}}\n",
        bounds.join(" + ")
    ));
    out
}

/// Scope lists are `'static` data like the rest of the marker; an empty list is
/// the trait default, so unscoped services are unchanged.
fn scope_items(service: &Service, op: &Operation) -> String {
    let list = |scopes: &[String]| {
        format!(
            "&[{}]",
            scopes
                .iter()
                .map(|scope| literal(scope))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let mut items = String::new();
    if !op.scopes.is_empty() {
        items.push_str(&format!(
            "    const SCOPES: &'static [&'static str] = {};\n",
            list(&op.scopes)
        ));
    }
    if !service.scopes.is_empty() && !op.auth.is_empty() {
        items.push_str(&format!(
            "    const SERVICE_SCOPES: &'static [&'static str] = {};\n",
            list(&service.scopes)
        ));
    }
    items
}

/// `IDEMPOTENCY_TOKEN` for an operation with an idempotency token, naming
/// the member as the model does and as the input's JSON form does, and
/// `OUTPUT_FROM_JSON` when its output does not stream, so a server extension
/// can answer a retry with an earlier attempt's output. Only these operations
/// get the reader: nothing else replays.
fn idempotency_items(service: &Service, op: &Operation, output_ty: &str) -> String {
    let Some(token) = &op.idempotency_token else {
        return String::new();
    };
    let wire = structure(service, op.input.as_ref())
        .and_then(|s| s.member(token))
        .map_or(token.as_str(), |m| m.wire_name.as_str());
    let mut out = format!(
        "    const IDEMPOTENCY_TOKEN: ::std::option::Option<::crucible_server_runtime_rust::__generated::operation::PaginationMember> = ::std::option::Option::Some(::crucible_server_runtime_rust::__generated::operation::PaginationMember {{ name: {}, wire_name: {} }});\n",
        literal(token),
        literal(wire)
    );
    let streams = op.response.payload.as_ref().is_some_and(|payload| {
        payload.streaming || payload.kind == crucible_smithy::ir::PayloadKind::EventStream
    });
    if !streams {
        out.push_str(&format!(
            "    const OUTPUT_FROM_JSON: ::std::option::Option<::crucible_server_runtime_rust::__generated::operation::OutputFromJson<Self::Output>> = ::std::option::Option::Some(::crucible_server_runtime_rust::__generated::serde_json::from_value::<{output_ty}>);\n"
        ));
    }
    out
}

/// `PAGINATION` for a `@paginated` operation, naming each input member as
/// the model does and as the input's JSON form does, so a server extension
/// can read the token without knowing the input's type.
fn pagination_items(service: &Service, op: &Operation) -> String {
    let Some(paginated) = &op.paginated else {
        return String::new();
    };
    let input = structure(service, op.input.as_ref());
    let member = |name: &str| {
        let wire = input
            .and_then(|s| s.member(name))
            .map_or(name, |m| m.wire_name.as_str());
        format!(
            "::crucible_server_runtime_rust::__generated::operation::PaginationMember {{ name: {}, wire_name: {} }}",
            literal(name),
            literal(wire)
        )
    };
    let page_size = paginated
        .page_size
        .as_deref()
        .map_or("::std::option::Option::None".to_owned(), |name| {
            format!("::std::option::Option::Some({})", member(name))
        });
    // crucible-smithy accepts a string or a map token.
    let token_map = input
        .and_then(|s| s.member(&paginated.input_token))
        .and_then(|m| service.shape(&m.target))
        .is_some_and(|shape| matches!(shape, Shape::Map(_)));
    let max_length = input
        .and_then(|s| s.member(&paginated.input_token))
        .and_then(|m| m.constraints.length.as_ref())
        .and_then(|length| length.max);
    let max_length = max_length.map_or("::std::option::Option::None".to_owned(), |max| {
        format!("::std::option::Option::Some({max})")
    });
    format!(
        "    const PAGINATION: ::std::option::Option<::crucible_server_runtime_rust::__generated::operation::Pagination> = ::std::option::Option::Some(::crucible_server_runtime_rust::__generated::operation::Pagination {{ input_token: {}, page_size: {page_size}, input_token_map: {token_map}, input_token_max_length: {max_length} }});
",
        member(&paginated.input_token)
    )
}
