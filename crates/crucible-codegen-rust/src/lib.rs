//! The Smithy adaptor for Rust: generates, from one service, the code that
//! plugs it into the crucible framework. `crucible-build` runs it from a
//! service's build script; the service includes the result with
//! `crucible::include_service!()`.
//!
//! The generated file holds:
//! - `model`: a type per shape, with serde and `Validate`
//! - `errors`: a struct per error, implementing `crucible::ModelledError`
//! - per operation `GetCity`: a marker type implementing `crucible::Operation`,
//!   and the trait `GetCityOperation` the service implements
//! - `SERVICE`, `operations(implementation)` and `builder()`
//!
//! `generate_client` writes the client side from the same model: `model` and
//! `errors` as above, the marker types, an error enum per operation and
//! a service-named client such as `WeatherClient`, all against `crucible-client`.

mod builders;
mod client;
mod errors;
mod events;
mod info;
mod models;
mod names;
mod open;
mod operations;
mod scaffold;
mod types;

pub use names::escape_identifier;
pub use scaffold::{ScaffoldOptions, Stub, file_stem, folder, module_name, scaffold};

use crucible_smithy::ir::{Service, Shape};
use crucible_smithy::{Diagnostics, ShapeId};

pub(crate) const DERIVE: &str = "#[derive(Debug, Clone, PartialEq, ::crucible::__private::serde::Serialize, ::crucible::__private::serde::Deserialize)]\n#[serde(crate = \"::crucible::__private::serde\")]\n";
pub(crate) const DERIVE_ENUM: &str = "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, ::crucible::__private::serde::Serialize, ::crucible::__private::serde::Deserialize)]\n#[serde(crate = \"::crucible::__private::serde\")]\n";

const ALLOW: &str = "#[allow(clippy::all, unused_imports, unused_variables, dead_code, non_snake_case, non_camel_case_types)]\n";

/// Which errors the generated code fills in itself. Both are optional:
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
    let validation = errors::validation_error(service, configured_validation);
    let internal = errors::internal_error(service, configured_internal);
    let reject = errors::reject(service, validation.as_ref());
    let internal_fn = errors::internal(service, internal.as_ref());
    if options.require_rejection && reject.is_none() && configured_validation.is_none() {
        diagnostics.push(&service.id,
            "the service needs a fillable validation error before it can start; \
             add `use crucible.errors#ValidationError` and `errors: [ValidationError]` \
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
        unknown: errors::unknown(service, validation.as_ref()),
        reject,
        internal: internal_fn,
    };
    let operations = format!(
        "{}{}{}",
        info::redactions(service),
        operations::emit(service, functions, &mut diagnostics),
        events::statics(service, &mut diagnostics, false)
    );
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(format!(
        "// Generated by crucible from {} {}. Do not edit: change the model.\n\n{ALLOW}pub mod model {{\n{}}}\n\n{ALLOW}pub mod errors {{\n{}}}\n{}",
        service.id,
        service.version,
        indent(&model),
        indent(&errors),
        operations
    ))
}

/// The generated client file's contents, for `crucible_client::include_client!()`.
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
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let code = format!(
        "// Generated by crucible from {} {}. Do not edit: change the model.\n\n{ALLOW}pub mod model {{\n{}}}\n\n{ALLOW}pub mod errors {{\n{}}}\n{}",
        service.id,
        service.version,
        indent(&model),
        indent(&errors),
        client
    );
    // Shared wire paths keep generated client declarations independent of the
    // server framework; client-only presence and open types live in the runtime.
    Ok(code.replace("::crucible::", "::crucible_client::"))
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
            if let Err(error) = regex::Regex::new(pattern) {
                diagnostics.push(
                    &id,
                    format!(
                        "@pattern {pattern:?} does not compile for the Rust regex crate: {error}"
                    ),
                );
            }
        }
    }
}
