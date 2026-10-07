//! The top level of the generated file: per operation a marker type, its
//! `OperationInfo`, and the trait the service implements; for the service its
//! `ServiceInfo` and the registration functions.

use crucible_smithy::ir::{Operation, Segment, Service, Shape};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::errors::{error_shapes, info_static};
use crate::info::{constant, custom_traits, members};
use crate::names::{doc, function, literal};

pub(crate) struct Functions {
    pub classify: String,
    pub reject: Option<String>,
    pub internal: Option<String>,
    pub unknown: Option<String>,
}

pub(crate) fn emit(
    service: &Service,
    functions: Functions,
    diagnostics: &mut Diagnostics,
) -> String {
    let mut out = String::new();
    for operation in &service.operations {
        out.push_str(&operation_items(service, operation));
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
    out.push_str(&service_info(service, &functions));
    out.push_str(&registration(service));
    out
}

fn operation_static(operation: &Operation) -> String {
    format!("{}_OPERATION", constant(operation.name()))
}

pub(crate) fn input_type(service: &Service, operation: &Operation) -> String {
    operation
        .input
        .as_ref()
        .map_or("::crucible::operation::Unit".to_owned(), |id| {
            format!("model::{}", service.name_of(id))
        })
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

fn operation_items(service: &Service, op: &Operation) -> String {
    let name = op.name();
    let mut out = marker(service, op, &output_type(service, op), false);
    doc(&mut out, "", op.docs.as_deref());
    let parameter = if op.input.is_some() {
        format!(", input: {}", input_type(service, op))
    } else {
        String::new()
    };
    out.push_str(&format!(
        "pub trait {name}Operation: ::crucible::WithContext + Send + Sync + 'static {{\n    fn {}(&self{parameter}) -> impl ::std::future::Future<Output = ::crucible::Result<{}>> + Send;\n}}\n",
        function(name),
        output_type(service, op),
    ));
    out
}

/// The marker type, its `OperationInfo` and its `Operation` impl, with
/// `output_ty` as the output type: a client reads a missing output as `Unit`.
pub(crate) fn marker(service: &Service, op: &Operation, output_ty: &str, client: bool) -> String {
    let name = op.name();
    let mut out = String::from("\n");
    doc(&mut out, "", op.docs.as_deref());
    out.push_str(&format!("pub struct {name};\n\n"));

    let path: Vec<String> = op
        .uri
        .segments
        .iter()
        .map(|segment| match segment {
            Segment::Literal(text) => {
                format!("::crucible::operation::Segment::Literal({})", literal(text))
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
                format!("::crucible::operation::Segment::{kind}({})", literal(wire))
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
        "static {}: ::crucible::OperationInfo = ::crucible::OperationInfo {{\n    id: {},\n    name: {},\n    resource: {resource},\n    method: {},\n    path: &[{}],\n    literal_query: &[{query}],\n    status: {},\n    input: {input},\n    output: {output},\n    errors: &[{}],\n    readonly: {},\n    idempotent: {},\n    idempotency_token: {token},\n    traits: {},\n}};\n\n",
        operation_static(op),
        literal(&op.id.to_string()),
        literal(name),
        literal(&op.method),
        path.join(", "),
        op.status,
        errors.join(", "),
        op.readonly,
        op.idempotent,
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
                "value.{rust}.value().map(|value| ::crucible::operation::response_status(i64::from(*value)))"
            )
        } else if !client && member.optional_on_server() {
            format!(
                "value.{rust}.as_ref().map(|value| ::crucible::operation::response_status(i64::from(*value)))"
            )
        } else {
            format!("Some(::crucible::operation::response_status(i64::from(value.{rust})))")
        };
        streaming.push_str(&format!(
            "    fn output_status(value: &Self::Output) -> Option<u16> {{ {get} }}\n"
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
                    "::crucible::Field::Value(stream)",
                )
            } else if !client && member.optional_on_server() {
                (format!("value.{rust}.clone()"), "Some(stream)")
            } else {
                (format!("Some(value.{rust}.clone())"), "stream")
            };
            streaming.push_str(&format!("    fn {direction}_stream(value: &Self::{ty}) -> Option<::crucible::ByteStream> {{ {get} }}\n    fn set_{direction}_stream(value: &mut Self::{ty}, stream: ::crucible::ByteStream) {{ value.{rust} = {set}; }}\n"));
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
            format!("::crucible::operation::AuthScheme::{value}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut auth = format!(
        "    const AUTH_SCHEMES: Option<&'static [::crucible::operation::AuthScheme]> = Some(&[{auth}]);\n    const OPTIONAL_AUTH: bool = {};\n",
        op.optional_auth
    );
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
    out.push_str(&format!(
        "impl ::crucible::Operation for {name} {{\n    const AUTH_BEARER: bool = {};\n{auth}    const INFO: &'static ::crucible::OperationInfo = &{};\n    type Input = {};\n    type Output = {};\n{streaming}}}\n\n",
        op.auth.iter().any(|id| id.to_string() == "smithy.api#httpBearerAuth"),
        operation_static(op),
        input_type(service, op),
        output_ty,
    ));
    out
}

/// A marker's protocol and checksum description, when they differ
/// from the defaults: `crucible.protocols#json` and no checksum.
fn wire(service: &Service, op: &Operation) -> String {
    use crucible_smithy::Protocol;
    use crucible_smithy::ir::{Checksum, ChecksumAlgorithm};
    let mut out = String::new();
    let protocol = match service.protocol {
        Protocol::AwsRestJson1 => Some("AwsRestJson1".to_owned()),
        Protocol::AwsJson10 => Some(format!(
            "AwsJson10 {{ service: {} }}",
            literal(service.id.name())
        )),
        Protocol::AwsJson11 => Some(format!(
            "AwsJson11 {{ service: {} }}",
            literal(service.id.name())
        )),
        Protocol::SmithyRpcV2Json => Some("SmithyRpcV2Json".to_owned()),
        Protocol::SmithyRpcV2Cbor => Some("SmithyRpcV2Cbor".to_owned()),
        _ => None,
    };
    if let Some(protocol) = protocol {
        out.push_str(&format!(
            "    const PROTOCOL: ::crucible::operation::Protocol = ::crucible::operation::Protocol::{protocol};\n"
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
                    format!("::crucible::operation::ChecksumAlgorithm::{name}")
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
        "    const CHECKSUM: Option<::crucible::operation::Checksum> = Some(::crucible::operation::Checksum::{checksum});\n"
    ));
    out
}

fn service_info(service: &Service, functions: &Functions) -> String {
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
    format!(
        "\n/// The service, as the framework sees it.\npub static SERVICE: ::crucible::ServiceInfo = ::crucible::ServiceInfo {{\n    id: {},\n    version: {},\n    operations: &[\n        {}{}\n    ],\n    errors: &[{}],\n    classify,\n    reject: {},\n    internal: {},\n    unknown: {},\n}};\n",
        literal(&service.id.to_string()),
        literal(&service.version),
        operations.join(",\n        "),
        if operations.is_empty() { "" } else { "," },
        errors.join(", "),
        option(functions.reject.is_some(), "reject"),
        option(functions.internal.is_some(), "internal"),
        option(functions.unknown.is_some(), "unknown"),
    )
}

fn registration(service: &Service) -> String {
    let mut out = String::from(
        "\n/// Registers implementations one operation at a time, for a service split\n/// across several types. `build` names any operation left out.\npub struct OperationsBuilder(::crucible::operations::OperationsBuilder);\n\nimpl OperationsBuilder {\n",
    );
    for op in &service.operations {
        let method = function(op.name());
        let call = if op.input.is_some() {
            format!(
                "|implementation, input| ::std::boxed::Box::pin(async move {{ implementation.{method}(input).await }})"
            )
        } else {
            format!(
                "|implementation, _| ::std::boxed::Box::pin(async move {{ implementation.{method}().await }})"
            )
        };
        out.push_str(&format!(
            "    /// Serves {name} with `implementation`.\n    pub fn {method}<S: {name}Operation>(self, implementation: ::std::sync::Arc<S>) -> Self {{\n        Self(self.0.add::<{name}, S>(implementation, {call}))\n    }}\n\n",
            name = op.name(),
        ));
    }
    out.push_str(
        "    pub fn build(self) -> ::std::result::Result<::crucible::Operations, ::crucible::MissingOperations> {\n        self.0.build()\n    }\n}\n\n",
    );
    out.push_str("pub fn builder() -> OperationsBuilder {\n    OperationsBuilder(::crucible::Operations::builder(&SERVICE))\n}\n\n");
    let bounds: Vec<String> = service
        .operations
        .iter()
        .map(|op| format!("{}Operation", op.name()))
        .collect();
    let calls: String = service
        .operations
        .iter()
        .map(|op| format!("\n        .{}(implementation.clone())", function(op.name())))
        .collect();
    let implementation = if service.operations.is_empty() {
        "_implementation"
    } else {
        "implementation"
    };
    out.push_str(&format!(
        "/// Every operation, served by one type: checked at compile time.\npub fn operations<T>({implementation}: ::std::sync::Arc<T>) -> ::crucible::Operations\nwhere\n    T: {},\n{{\n    builder(){calls}\n        .build()\n        .expect(\"every operation is registered\")\n}}\n",
        bounds.join(" + ")
    ));
    out
}
