//! The Smithy adaptor for Rust: generates, from one service, the code that
//! plugs it into the crucible framework. `crucible-build` runs it from a
//! service's build script; the service includes the result with
//! `crucible_server_runtime_rust::include_service!()`.
//!
//! The generated file holds:
//! - a service-named module containing every shape, exception and operation
//! - private implementation modules for serde and `Validate`
//! - exception structs implementing `crucible_server_runtime_rust::ModelledError`
//! - per operation `GetCity`: a marker type implementing `crucible_server_runtime_rust::Operation`,
//!   and the trait `GetCityOperation` the service implements
//! - `SERVICE`, `operations(implementation)` and `builder()`
//!
//! `generate_client` writes the client side from the same model: flat public
//! models and exceptions, the marker types, an error enum per operation and
//! a service-named client such as `WeatherClient`, all against `crucible-client-runtime-rust`.

mod builders;
mod client;
mod client_api;
mod client_options;
mod client_package;
mod client_permissions;
mod conformance;
mod debug;
mod errors;
mod events;
mod file;
mod info;
mod mcp;
mod models;
mod names;
mod open;
mod operations;
mod permissions;
mod runtime_dependency;
mod scaffold;
mod server_api;
mod types;
mod versions;

pub use client::pages_method;
pub use client_options::ClientOptions;
pub use client_package::{client_package_name, emit_client_package};
pub use conformance::{ConformanceOptions, Seed, Skip, generate_conformance};
pub use file::File;
pub use mcp::exclusions as mcp_exclusions;
pub use names::escape_identifier;
pub use runtime_dependency::RuntimeDependency;
pub use scaffold::{ScaffoldOptions, Stub, file_stem, folder, module_name, scaffold};

use crucible_smithy::ir::{Service, Shape};
use crucible_smithy::{Diagnostics, ShapeId};

/// Verified Crucible Git revision embedded when this emitter was built.
/// Packaged or vendored emitters without source provenance return `None`.
pub fn source_revision() -> Option<&'static str> {
    crucible_revision::source_revision()
}

pub(crate) const DERIVE: &str = "#[derive(Debug, Clone, PartialEq, ::crucible_server_runtime_rust::__generated::serde::Serialize, ::crucible_server_runtime_rust::__generated::serde::Deserialize)]\n#[serde(crate = \"::crucible_server_runtime_rust::__generated::serde\")]\n";
/// On every client type: a consumer in another crate builds structures
/// through their builders and matches with a `_` arm, so a member, union
/// arm or enum value a newer model adds breaks no consumer's source.
pub(crate) const NON_EXHAUSTIVE: &str = "#[non_exhaustive]\n";
pub(crate) const DERIVE_ENUM: &str = "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, ::crucible_server_runtime_rust::__generated::serde::Serialize, ::crucible_server_runtime_rust::__generated::serde::Deserialize)]\n#[serde(crate = \"::crucible_server_runtime_rust::__generated::serde\")]\n";

const ALLOW: &str = "#[allow(clippy::all, unused_imports, unused_variables, dead_code, non_snake_case, non_camel_case_types, deprecated)]\n";

/// Which errors the generated code fills in itself. These are optional:
/// without a validation error crucible can fill, the service gives the
/// builder `on_rejection`; without an internal error, internal failures are
/// answered with the protocol's own `InternalFailure`.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Generate External-audience MCP tool descriptors. Serving remains opt-in.
    pub mcp: bool,
    /// Generate an embedded Explorer description for this audience.
    pub explorer: Option<crucible_smithy::Audience>,
    /// The error a rejected request becomes. Unset, the service's one error
    /// named `*Validation*`, if it has exactly one. Set, it must be an error
    /// crucible can fill (`message`, and a `fieldList` of `path`/`name` +
    /// `message`), or generation fails.
    pub validation_error: Option<ShapeId>,
    /// The error an internal failure is answered with. Unset, the service's
    /// one server error with status 500, if it has exactly one. Set, it must
    /// be an error whose only required member is `message`, or generation
    /// fails.
    pub internal_error: Option<ShapeId>,
    /// The error returned when no operation matches. Unset, uses the reachable
    /// `crucible.errors#UnknownOperationException` shape. A configured error must
    /// be a client error with status 404 whose required members Crucible can fill.
    /// Without it, the validation error's `unknownOperation` reason or the
    /// protocol's own unknown-operation response is used.
    pub unknown_operation_error: Option<ShapeId>,
    /// Every structure an input reaches refuses members it does not define.
    /// Without it, only structures with `@crucible.protocols#strict` do.
    pub strict_inputs: bool,
    /// Fail generation if the model cannot supply a request rejection error.
    /// Enabled by the single-crate Smithy build helper. Advanced services that
    /// provide their own `on_rejection` handler leave this disabled.
    pub require_rejection: bool,
}

/// The generated file's contents.
pub fn capabilities() -> crucible_smithy::Capabilities {
    use crucible_smithy::Feature;
    crucible_smithy::Capabilities::new([
        Feature::JsonName,
        Feature::GreedyLabels,
        Feature::QueryParams,
        Feature::PrefixHeaders,
        Feature::ResponseCode,
        Feature::Streaming,
        Feature::EventStreams,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::Sparse,
        Feature::Sets,
        Feature::BasicAuth,
        Feature::ApiKeyAuth,
        Feature::OptionalAuth,
        Feature::SigV4,
        Feature::NonBlobPayload,
        Feature::ErrorPayload,
        Feature::MediaTypeHeaders,
        Feature::AwsRestJson1,
        Feature::AwsJson10,
        Feature::AwsJson11,
        Feature::SmithyRpcV2Json,
        Feature::SmithyRpcV2Cbor,
    ])
}

/// Constructs supported by the generated client and its bundled runtime.
pub fn client_capabilities() -> crucible_smithy::Capabilities {
    use crucible_smithy::Feature;
    crucible_smithy::Capabilities::new(capabilities().iter().chain([
        Feature::EndpointHostPrefix,
        Feature::EndpointRules,
        Feature::Waiters,
        Feature::HttpChecksum,
    ]))
}

pub fn generate(service: &Service, options: &Options) -> Result<String, Diagnostics> {
    generate_with_explorer_model(service, options, service)
}

/// Generate server bindings while describing a separately lowered documentation model.
pub fn generate_with_explorer_model(
    service: &Service,
    options: &Options,
    explorer_service: &Service,
) -> Result<String, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    for version in &service.versions {
        version.service.require_capabilities(&capabilities())?;
    }
    let mut diagnostics = Diagnostics::default();
    let body = body(service, options, explorer_service, &mut diagnostics);
    let versions = versions::emit(service, options, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(format!(
        "// Generated by crucible from {} {}. Do not edit: change the model.\n\n::crucible_server_runtime_rust::__generated::check_abi!(\"{}\");\n\n/// Server API for `{}`.\npub mod {} {{\n{}}}\n",
        service.id,
        service.version,
        env!("CARGO_PKG_VERSION"),
        service.id,
        server_api::module(service),
        indent(&format!("{body}{versions}")),
    ))
}

/// A service's models, errors, operations and registration: the server
/// module's contents, for the current version and each older one.
pub(crate) fn body(
    service: &Service,
    options: &Options,
    explorer_service: &Service,
    diagnostics: &mut Diagnostics,
) -> String {
    body_with(service, options, explorer_service, true, false, diagnostics)
}

/// [`body`], with `strict` false for an older release. Its output types read
/// the current version's values converted down, which may carry members
/// added since, so a structure an output or error reaches never refuses an
/// unknown member; one only inputs reach decodes as strictly as before.
///
/// `parent_permissions`: the module states requirements against the
/// service module's `permissions`, two levels up (an older release's
/// legacy operations), instead of generating its own.
pub(crate) fn body_with(
    service: &Service,
    options: &Options,
    explorer_service: &Service,
    strict: bool,
    parent_permissions: bool,
    diagnostics: &mut Diagnostics,
) -> String {
    let mut strict_shapes = models::strict_shapes(service, options.strict_inputs);
    if !strict {
        let outputs = models::output_reachable(service);
        strict_shapes.retain(|id| !outputs.contains(id));
    }
    let strict = strict_shapes;
    let model = models::emit(service, &strict, false, diagnostics);
    let errors = errors::emit(service, false, diagnostics);
    check_patterns(service, diagnostics);
    let configured_validation = options.validation_error.as_ref();
    let configured_internal = options.internal_error.as_ref();
    let configured_unknown = options.unknown_operation_error.as_ref();
    let validation_ok = errors::configured_error(
        service,
        configured_validation,
        "validation error",
        diagnostics,
    );
    let internal_ok =
        errors::configured_error(service, configured_internal, "internal error", diagnostics);
    let unknown_ok = errors::configured_error(
        service,
        configured_unknown,
        "unknown-operation error",
        diagnostics,
    );
    let validation = errors::validation_error(service, configured_validation);
    let internal = errors::internal_error(service, configured_internal);
    let unknown = errors::unknown_operation_error(service, configured_unknown);
    let reject = errors::reject(service, validation.as_ref());
    let internal_fn = errors::internal(service, internal.as_ref());
    let unknown_fn = errors::unknown_operation(service, unknown.as_ref(), diagnostics);
    if let (Some(id), true, None) = (&unknown, unknown_ok, &unknown_fn) {
        diagnostics.push(
            id,
            "the unknown-operation error has required members besides `message` and `requestId`",
        );
    }
    if options.require_rejection && reject.is_none() && configured_validation.is_none() {
        diagnostics.push(&service.id,
            "the service needs a fillable validation error before it can start; \
             add `use crucible.errors#ValidationException` and `errors: [ValidationException]` \
             to the service, or call `.validation_error(\"namespace#Error\")` for a declared \
             custom error; services with their own `on_rejection` handler can use `.custom_rejections()`");
    }
    if let (Some(id), true, None) = (configured_validation, validation_ok, &reject) {
        let recovery = if options.require_rejection {
            "leave it unset, call `.custom_rejections()`, and give the builder on_rejection instead"
        } else {
            "leave it unset and give the builder on_rejection instead"
        };
        diagnostics.push(
            id,
            format!("the configured validation error has required members crucible cannot fill; {recovery}"),
        );
    }
    if let (Some(id), true, None) = (configured_internal, internal_ok, &internal_fn) {
        diagnostics.push(
            id,
            "the configured internal error has required members besides `message`",
        );
    }
    let functions = operations::Functions {
        classify: errors::classify(service),
        unknown: if unknown.is_some() {
            unknown_fn
        } else {
            errors::unknown(service, validation.as_ref())
        },
        reject,
        internal: internal_fn,
    };
    let operations = format!(
        "{}{}{}{}",
        info::redactions(service),
        operations::emit(service, functions, options.mcp, diagnostics),
        events::statics(service, diagnostics, false),
        if parent_permissions {
            "\n#[allow(unused_imports)]\nuse super::super::permissions;\n".to_owned()
        } else {
            permissions::module(service, diagnostics)
        },
    );
    let exports = server_api::exports(service, diagnostics);
    let mcp = if options.mcp {
        mcp::emit(service, &strict, diagnostics)
    } else {
        String::new()
    };
    let explorer = match options.explorer {
        Some(audience) => {
            let description =
                crucible_smithy::project(explorer_service, audience).and_then(|projected| {
                    crucible_codegen_common::describe::describe_for_explorer(&projected)
                });
            match description {
                Ok(description) => {
                    let description = description.to_json();
                    // Parse JSON as data, preserving keys such as __proto__ rather than
                    // giving them JavaScript object-literal semantics.
                    let script = format!(
                        "globalThis.CRUCIBLE_DESCRIPTION = JSON.parse({});\n",
                        serde_json::to_string(&description).expect("JSON text")
                    );
                    format!(
                        "\n/// Embedded Explorer contract, generated at build time.\npub static DESCRIPTION: &::crucible_server_runtime_rust::explorer::Description = &::crucible_server_runtime_rust::__generated::explorer_description({}, {});\n",
                        names::literal(&description),
                        names::literal(&script)
                    )
                }
                Err(errors) => {
                    // Description refusals must not hide ordinary server diagnostics.
                    for error in errors.iter() {
                        diagnostics.push(&error.shape, &error.message);
                    }
                    String::new()
                }
            }
        }
        None => String::new(),
    };
    format!(
        "{ALLOW}mod model {{\n{}}}\n\n{ALLOW}mod errors {{\n{}}}\n\n{exports}\n{operations}{mcp}{explorer}",
        indent(&model),
        indent(&errors),
    )
}

/// The generated client file's contents, for `crucible_client_runtime_rust::include_client!()`.
pub fn generate_client(service: &Service) -> Result<String, Diagnostics> {
    service.require_capabilities(&client_capabilities())?;
    let mut diagnostics = Diagnostics::default();
    // A client reads what a newer service sends: never strict.
    let model = models::emit(service, &Default::default(), true, &mut diagnostics);
    let errors = errors::emit(service, true, &mut diagnostics);
    check_patterns(service, &mut diagnostics);
    let client = format!(
        "{}{}{}",
        info::redactions(service),
        client::emit(service, &mut diagnostics),
        client_permissions::module(service, &mut diagnostics),
    );
    let exports = client_api::exports(service, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let code = format!(
        "// Generated by crucible from {} {}. Do not edit: change the model.\n\n::crucible_server_runtime_rust::__generated::check_abi!(\"{}\");\n\n{ALLOW}mod model {{\n{}}}\n\n{ALLOW}mod errors {{\n{}}}\n\n{exports}\n{}",
        service.id,
        service.version,
        env!("CARGO_PKG_VERSION"),
        indent(&model),
        indent(&errors),
        client
    );
    // Shared wire paths keep generated client declarations independent of the
    // server framework; client-only presence and open types live in the runtime.
    Ok(code.replace(
        "::crucible_server_runtime_rust::__generated::",
        "::crucible_client_runtime_rust::__generated::",
    ))
}

fn indent(code: &str) -> String {
    code.lines()
        .map(|line| {
            if line.is_empty() {
                "\n".to_owned()
            } else {
                format!("    {line}\n")
            }
        })
        .collect()
}

/// Every `@pattern` must compile under the regex crate the framework runs,
/// which lacks look-around and backreferences.
fn check_patterns(service: &Service, diagnostics: &mut Diagnostics) {
    let mut members = Vec::new();
    for (id, shape) in &service.shapes {
        match shape {
            Shape::Structure(s) | Shape::Union(s) => {
                members.extend(s.members.iter().map(|m| (id.with_member(&m.name), m)));
            }
            Shape::List(l) => members.push((id.with_member("member"), &l.member)),
            Shape::Map(m) => {
                members.push((id.with_member("key"), &m.key));
                members.push((id.with_member("value"), &m.value));
            }
            _ => {}
        }
    }
    for (id, member) in members {
        if let Some(pattern) = &member.constraints.pattern {
            let checked = member.constraints.rust_pattern().and_then(|translated| {
                regex::Regex::new(&translated.expect("pattern is present"))
                    .map_err(|error| error.to_string())
            });
            if let Err(error) = checked {
                diagnostics.push(
                    &id,
                    format!(
                        "@pattern {pattern:?} does not compile faithfully for the Rust regex crate: {error}"
                    ),
                );
            }
        }
    }
}
