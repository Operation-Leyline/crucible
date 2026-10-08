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
mod conformance;
mod debug;
mod errors;
mod events;
mod file;
mod info;
mod models;
mod names;
mod open;
mod operations;
mod runtime_dependency;
mod scaffold;
mod server_api;
mod types;

pub use client::pages_method;
pub use client_options::ClientOptions;
pub use client_package::emit_client_package;
pub use conformance::{ConformanceOptions, Seed, Skip, generate_conformance};
pub use file::File;
pub use names::escape_identifier;
pub use runtime_dependency::RuntimeDependency;
pub use scaffold::{ScaffoldOptions, Stub, file_stem, folder, module_name, scaffold};

use crucible_smithy::ir::{Service, Shape};
use crucible_smithy::{Diagnostics, ShapeId};

/// Verified Crucible Git revision embedded when this emitter was built.
/// Packaged or vendored emitters without source provenance return `None`.
pub fn source_revision() -> Option<&'static str> {
    option_env!("CRUCIBLE_GENERATOR_REVISION")
}

pub(crate) const DERIVE: &str = "#[derive(Debug, Clone, PartialEq, ::crucible_server_runtime_rust::__private::serde::Serialize, ::crucible_server_runtime_rust::__private::serde::Deserialize)]\n#[serde(crate = \"::crucible_server_runtime_rust::__private::serde\")]\n";
pub(crate) const DERIVE_ENUM: &str = "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, ::crucible_server_runtime_rust::__private::serde::Serialize, ::crucible_server_runtime_rust::__private::serde::Deserialize)]\n#[serde(crate = \"::crucible_server_runtime_rust::__private::serde\")]\n";

const ALLOW: &str = "#[allow(clippy::all, unused_imports, unused_variables, dead_code, non_snake_case, non_camel_case_types)]\n";

/// Which errors the generated code fills in itself. These are optional:
/// without a validation error crucible can fill, the service gives the
/// builder `on_rejection`; without an internal error, internal failures are
/// answered with the protocol's own `InternalFailure`.
#[derive(Debug, Clone, Default)]
pub struct Options {
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
    service.require_capabilities(&capabilities())?;
    let mut diagnostics = Diagnostics::default();
    let strict = models::strict_shapes(service, options.strict_inputs);
    let model = models::emit(service, &strict, false, &mut diagnostics);
    let errors = errors::emit(service, false, &mut diagnostics);
    check_patterns(service, &mut diagnostics);
    let configured_validation = options.validation_error.as_ref();
    let configured_internal = options.internal_error.as_ref();
    let configured_unknown = options.unknown_operation_error.as_ref();
    let validation_ok = errors::configured_error(
        service,
        configured_validation,
        "validation error",
        &mut diagnostics,
    );
    let internal_ok = errors::configured_error(
        service,
        configured_internal,
        "internal error",
        &mut diagnostics,
    );
    let unknown_ok = errors::configured_error(
        service,
        configured_unknown,
        "unknown-operation error",
        &mut diagnostics,
    );
    let validation = errors::validation_error(service, configured_validation);
    let internal = errors::internal_error(service, configured_internal);
    let unknown = errors::unknown_operation_error(service, configured_unknown);
    let reject = errors::reject(service, validation.as_ref());
    let internal_fn = errors::internal(service, internal.as_ref());
    let unknown_fn = errors::unknown_operation(service, unknown.as_ref(), &mut diagnostics);
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
        "{}{}{}",
        info::redactions(service),
        operations::emit(service, functions, &mut diagnostics),
        events::statics(service, &mut diagnostics, false)
    );
    let exports = server_api::exports(service, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let body = format!(
        "{ALLOW}mod model {{\n{}}}\n\n{ALLOW}mod errors {{\n{}}}\n\n{exports}\n{operations}",
        indent(&model),
        indent(&errors),
    );
    Ok(format!(
        "// Generated by crucible from {} {}. Do not edit: change the model.\n\n/// Server API for `{}`.\npub mod {} {{\n{}}}\n",
        service.id,
        service.version,
        service.id,
        server_api::module(service),
        indent(&body),
    ))
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
        "{}{}",
        info::redactions(service),
        client::emit(service, &mut diagnostics)
    );
    let exports = client_api::exports(service, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let code = format!(
        "// Generated by crucible from {} {}. Do not edit: change the model.\n\n{ALLOW}mod model {{\n{}}}\n\n{ALLOW}mod errors {{\n{}}}\n\n{exports}\n{}",
        service.id,
        service.version,
        indent(&model),
        indent(&errors),
        client
    );
    // Shared wire paths keep generated client declarations independent of the
    // server framework; client-only presence and open types live in the runtime.
    Ok(code.replace(
        "::crucible_server_runtime_rust::",
        "::crucible_client_runtime_rust::",
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
