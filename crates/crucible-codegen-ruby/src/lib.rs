//! Ruby gem generation from the shared, lowered Smithy IR.
mod models;
mod names;

use crucible_codegen_common::docs;
use crucible_smithy::{
    Diagnostics,
    ir::{Service, Shape},
};
use names::{constant, field, literal};

fn client_docs(service: &Service, client_name: &str) -> String {
    let mut text = format!(
        "Client for the {} service.\n\n",
        service
            .title
            .as_deref()
            .unwrap_or_else(|| service.id.name())
    );
    if let Some(service_docs) = &service.docs {
        text.push_str(&docs::escape(service_docs));
        text.push_str("\n\n");
    }
    text.push_str(&format!(
        "Create one `{client_name}` and share it: it is safe to call from many threads at once, and it keeps its HTTP connection open between calls. Call {{#close}} when finished. Every operation blocks until it completes and reports failure as a `ServiceException` for a service response, a {{Runtime::NetworkError}} when no response arrives, a {{Runtime::DeadlineError}} when the call outlasts {{Runtime::ClientConfig#timeout}}, or a {{Runtime::CancelledError}}."
    ));
    docs::line_comment(
        "  ",
        "#",
        &text.replace(
            "ServiceException",
            &crucible_codegen_common::service_exception_name(service),
        ),
    )
}

fn operation_docs(
    service: &Service,
    op: &crucible_smithy::ir::Operation,
    input: Option<&str>,
    output: &str,
    streaming: bool,
    pages: bool,
) -> String {
    let token = op.idempotency_token.as_deref().map(field);
    let mut text = String::new();
    if let Some(summary) = &op.docs {
        text.push_str(&docs::escape(summary));
        text.push_str("\n\n");
    }
    text.push_str(&docs::operation_notes(op, token.as_deref()).join("\n\n"));
    if pages {
        text.push_str(
            "\n\nThis operation is paginated: use the `_pages` variant to read every page.",
        );
    }
    text.push('\n');
    if let Some(input) = input {
        text.push_str(&format!("\n@param input [{input}] the request to send"));
    }
    if streaming {
        text.push_str("\n@param response_target [IO, nil] receives the streamed body; the body is yielded to the block when omitted");
        text.push_str("\n@yieldparam chunk [String] a chunk of the streamed body");
    }
    text.push_str(
        "\n@param cancellation [Runtime::CancellationToken, nil] cancels the call when cancelled",
    );
    text.push_str(&format!("\n@return [{output}] the service's response"));
    for name in docs::operation_errors(service, op) {
        text.push_str(&format!("\n@raise [{name}]"));
    }
    text.push_str("\n@raise [Runtime::ServiceException] for any other error response");
    docs::line_comment(
        "    ",
        "#",
        &text.replace(
            "ServiceException",
            &crucible_codegen_common::service_exception_name(service),
        ),
    )
}
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub struct Options {
    pub package_name: String,
    pub module_name: String,
    pub package_version: String,
    pub endpoint: Option<String>,
    pub content_sha256: bool,
}
pub struct File {
    pub path: String,
    pub contents: String,
}

fn diagnose(service: &Service, options: &Options) -> Diagnostics {
    let mut diagnostics = Diagnostics::default();
    if !options
        .package_name
        .starts_with(|c: char| c.is_ascii_lowercase())
        || !options
            .package_name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        diagnostics.push(
            &service.id,
            "Ruby package name must be a lowercase require identifier",
        );
    }
    if !names::root_module(&options.module_name) {
        diagnostics.push(&service.id, "Ruby module name must be a valid constant");
    }
    let mut files = BTreeSet::new();
    let mut constants = BTreeSet::from(["Registry", "Runtime"].map(str::to_owned));
    let client_name = format!("{}Client", service.id.name());
    if !constant(&client_name) {
        diagnostics.push(
            &service.id,
            "Ruby service client name must be a valid constant",
        );
    }
    constants.insert(client_name);
    for (id, shape) in &service.shapes {
        if !matches!(
            shape,
            Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_)
        ) {
            continue;
        }
        let name = service.name_of(id);
        if !constant(name) || !constants.insert(name.to_owned()) || !files.insert(field(name)) {
            diagnostics.push(id, "invalid or colliding Ruby model name");
        }
        match shape {
            Shape::Structure(structure) => {
                let mut members = BTreeSet::new();
                for member in &structure.members {
                    let name = field(&member.name);
                    if !names::method(&name) || !members.insert(name) {
                        diagnostics
                            .push(&id.with_member(&member.name), "Ruby member name collision");
                    }
                }
            }
            Shape::Union(union) => {
                let mut variants = BTreeSet::from(["Unknown".to_owned()]);
                for member in &union.members {
                    let name = models::variant(&member.name);
                    if !constant(&name) || !variants.insert(name) {
                        diagnostics.push(
                            &id.with_member(&member.name),
                            "Ruby union variant name collision",
                        );
                    }
                }
            }
            Shape::Enum(_) | Shape::IntEnum(_) => {
                let mut values = BTreeSet::new();
                let constants = match shape {
                    Shape::Enum(e) => e.values.iter().map(|value| &value.name).collect::<Vec<_>>(),
                    Shape::IntEnum(e) => e.values.iter().map(|value| &value.name).collect(),
                    _ => unreachable!(),
                };
                for value in constants {
                    let name = models::enum_constant(value);
                    if !constant(&name) || !values.insert(name) {
                        diagnostics.push(id, "Ruby enum constant name collision");
                    }
                }
            }
            _ => {}
        }
    }
    let mut methods = BTreeSet::from(["close".to_owned(), "call".to_owned(), "pages".to_owned()]);
    for op in &service.operations {
        let name = field(op.name());
        for suffix in std::iter::once("").chain(op.paginated.is_some().then_some("_pages")) {
            let method = format!("{name}{suffix}");
            if !names::method(&method) || !methods.insert(method) {
                diagnostics.push(&op.id, "Ruby operation name collision");
            }
        }
        for waiter in op.waiters.keys() {
            let method = waiter_method(waiter);
            if !names::method(&method) || !methods.insert(method) {
                diagnostics.push(&op.id, "Ruby waiter method name collision");
            }
        }
    }
    diagnostics
}

fn waiter_method(name: &str) -> String {
    format!("wait_until_{}", field(name))
}

pub fn capabilities() -> crucible_smithy::Capabilities {
    use crucible_smithy::Feature;
    crucible_smithy::Capabilities::new([
        Feature::NonBlobPayload,
        Feature::MediaTypeHeaders,
        Feature::ErrorPayload,
        Feature::BasicAuth,
        Feature::ApiKeyAuth,
        Feature::OptionalAuth,
        Feature::SigV4,
        Feature::JsonName,
        Feature::GreedyLabels,
        Feature::QueryParams,
        Feature::PrefixHeaders,
        Feature::ResponseCode,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::Sparse,
        Feature::Sets,
        Feature::Waiters,
        Feature::EndpointRules,
        Feature::EndpointHostPrefix,
        Feature::AwsRestJson1,
        Feature::AwsJson10,
        Feature::AwsJson11,
        Feature::HttpChecksum,
        Feature::Streaming,
        Feature::AwsRestXml,
        Feature::AwsQuery,
        Feature::Ec2Query,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    crucible_codegen_common::check_service_exception_name(service)?;
    let diagnostics = diagnose(service, options);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let client_name = format!("{}Client", service.id.name());
    let package = &options.package_name;
    let module = &options.module_name;
    let schema =
        serde_json::to_string(&crucible_codegen_common::schema(service, field)).expect("schema");
    let mut files = models::emit(service, options);
    files.push(File {
        path: format!("lib/{package}/schema.json"),
        contents: schema.clone(),
    });
    files.push(File {
        path: "crucible.json".into(),
        contents: serde_json::to_string_pretty(&serde_json::json!({
            "generatorVersion":env!("CARGO_PKG_VERSION"), "service":service.id.to_string(),
            "serviceVersion":service.version,"packageVersion":options.package_version,
            "schemaSha256":format!("{:x}",Sha256::digest(schema.as_bytes()))
        }))
        .expect("provenance"),
    });
    files.push(File {path:format!("{package}.gemspec"), contents:format!("# frozen_string_literal: true\nGem::Specification.new do |spec|\n  spec.name = {}\n  spec.version = {}\n  spec.summary = {}\n  spec.authors = ['Package maintainers']\n  spec.required_ruby_version = '>= 3.3', '< 5'\n  spec.files = Dir['lib/**/*', 'sig/**/*', 'crucible.json', '.yardopts']\n  spec.require_paths = ['lib']\n  spec.add_dependency 'json', '>= 2.7', '< 3'\n  spec.add_dependency 'net-http', '>= 0.4', '< 1'\n  spec.add_dependency 'base64', '>= 0.2', '< 1'\n  spec.add_dependency 'date', '>= 3.3', '< 4'\n  spec.add_dependency 'time', '>= 0.3', '< 1'\n  spec.add_dependency 'securerandom', '>= 0.3', '< 1'\n  spec.add_dependency 'openssl', '>= 3.0', '< 5'\n  spec.add_dependency 'digest', '>= 3.1', '< 4'\n  spec.add_dependency 'bigdecimal', '>= 3.1', '< 5'\n  spec.add_dependency 'zlib', '>= 3.0', '< 4'\n  spec.add_dependency 'ipaddr', '>= 1.2', '< 2'\n  spec.add_dependency 'stringio', '>= 3.0', '< 4'\n  spec.add_dependency 'rexml', '>= 3.2', '< 4'\nend\n",literal(package),literal(&options.package_version),literal(&format!("Generated {} client",service.name_of(&service.id))))});
    // The doc comments are Markdown (Smithy's @documentation is CommonMark).
    files.push(File {
        path: ".yardopts".into(),
        contents: "--markup markdown\n--no-private\nlib/**/*.rb\n".into(),
    });
    let mut entry = String::from("# frozen_string_literal: true\n");
    for (name, source) in [
        ("unit", include_str!("../runtime/unit.rb")),
        ("describe", include_str!("../runtime/describe.rb")),
        ("json_number", include_str!("../runtime/json_number.rb")),
        ("exact_json", include_str!("../runtime/exact_json.rb")),
        ("json_value", include_str!("../runtime/json_value.rb")),
        ("timestamp", include_str!("../runtime/timestamp.rb")),
        ("wire_context", include_str!("../runtime/wire_context.rb")),
        ("wire_codec", include_str!("../runtime/wire_codec.rb")),
        (
            "error_metadata",
            include_str!("../runtime/error_metadata.rb"),
        ),
        ("service_error", include_str!("../runtime/service_error.rb")),
        (
            "cancellation_token",
            include_str!("../runtime/cancellation_token.rb"),
        ),
        ("call_scope", include_str!("../runtime/call_scope.rb")),
        (
            "basic_credentials",
            include_str!("../runtime/basic_credentials.rb"),
        ),
        (
            "aws_credentials",
            include_str!("../runtime/aws_credentials.rb"),
        ),
        (
            "cached_credentials",
            include_str!("../runtime/cached_credentials.rb"),
        ),
        ("credentials", include_str!("../runtime/credentials.rb")),
        (
            "authentication",
            include_str!("../runtime/authentication.rb"),
        ),
        ("byte_stream", include_str!("../runtime/byte_stream.rb")),
        ("query", include_str!("../runtime/query.rb")),
        ("waiters", include_str!("../runtime/waiters.rb")),
        ("endpoints", include_str!("../runtime/endpoints.rb")),
        ("checksums", include_str!("../runtime/checksums.rb")),
        ("xml_protocol", include_str!("../runtime/xml_protocol.rb")),
        ("client_config", include_str!("../runtime/client_config.rb")),
        ("http_request", include_str!("../runtime/http_request.rb")),
        ("http_response", include_str!("../runtime/http_response.rb")),
        (
            "net_http_transport",
            include_str!("../runtime/net_http_transport.rb"),
        ),
        (
            "client_runtime",
            include_str!("../runtime/client_runtime.rb"),
        ),
    ] {
        entry.push_str(&format!("require_relative '{package}/runtime/{name}'\n"));
        files.push(File {
            path: format!("lib/{package}/runtime/{name}.rb"),
            contents: crucible_codegen_common::service_exception_runtime(service, source)
                .replace("CrucibleGenerated", module),
        });
    }
    let exception = crucible_codegen_common::service_exception_name(service);
    entry.push_str(&format!(
        "module {module}\n  {exception} = Runtime::{exception}\nend\n"
    ));
    for file in &files {
        if let Some(model) = file.path.strip_prefix(&format!("lib/{package}/models/")) {
            entry.push_str(&format!(
                "require_relative '{package}/models/{}'\n",
                model.trim_end_matches(".rb")
            ));
        }
    }
    entry.push_str(&format!("module {module}\n  def self.codec\n    @codec ||= Runtime::WireCodec.new(Runtime::ExactJson.decode(::File.read(::File.join(__dir__, '{package}/schema.json'))), Registry::TYPES)\n  end\nend\n"));
    entry.push_str(&format!("require_relative '{package}/client'\n"));
    let mut client = format!(
        "# frozen_string_literal: true\nmodule {module}\n{}  class {client_name}\n{}    def initialize(config = Runtime::ClientConfig.new)\n      @runtime = Runtime::ClientRuntime.new(config, ::{module}.codec, endpoint: {}, digest: {})\n    end\n    # Releases the HTTP connection this client created. A transport passed in the configuration is left open.\n    def close = @runtime.close\n",
        client_docs(service, &client_name),
        docs::line_comment(
            "    ",
            "#",
            "Creates a client. Without a `config`, the generated endpoint is used and no credentials are sent; see {Runtime::ClientConfig} for credentials, retries, timeouts and the transport.\n\n@param config [Runtime::ClientConfig]\n@raise [ArgumentError] when the endpoint or another setting is invalid"
        ),
        literal(options.endpoint.as_deref().unwrap_or("")),
        options.content_sha256
    );
    for op in &service.operations {
        let name = field(op.name());
        let input = if op.input.is_some() { "input, " } else { "" };
        let value = if op.input.is_some() {
            "input"
        } else {
            "Runtime::Unit.new"
        };
        let streaming = op
            .response
            .payload
            .as_ref()
            .is_some_and(|payload| payload.streaming);
        let input_type = op.input.as_ref().map(|id| service.name_of(id).to_string());
        let output_type = op.output.as_ref().map_or_else(
            || "Runtime::Unit".to_string(),
            |id| service.name_of(id).to_string(),
        );
        client.push_str(&operation_docs(
            service,
            op,
            input_type.as_deref(),
            &output_type,
            streaming,
            op.paginated.is_some(),
        ));
        if streaming {
            client.push_str(&format!("    def {name}({input}response_target: nil, cancellation: nil, &block)\n      @runtime.call({}, {value}, cancellation: cancellation, response_target: response_target, block: block)\n    end\n",literal(op.name())));
        } else {
            client.push_str(&format!("    def {name}({input}cancellation: nil)\n      @runtime.call({}, {value}, cancellation: cancellation)\n    end\n",literal(op.name())));
        }
        if op.paginated.is_some() {
            client.push_str(&docs::line_comment("    ", "#", &format!("Every page of {{#{name}}}, following the pagination token until the service stops returning one. Returns an `Enumerator`; the call for each page is made as it is consumed, so each can fail.\n\n@param input [{}] the request for the first page\n@param cancellation [Runtime::CancellationToken, nil]\n@return [Enumerator<{output_type}>]", input_type.as_deref().unwrap_or("Runtime::Unit"))));
            client.push_str(&format!("    def {name}_pages(input, cancellation: nil)\n      @runtime.pages({}, input, cancellation: cancellation)\n    end\n",literal(op.name())));
        }
        for (waiter, description) in &op.waiters {
            let waiter_docs = description
                .documentation
                .as_deref()
                .map(|text| {
                    format!(
                        "{}

",
                        docs::escape(text)
                    )
                })
                .unwrap_or_default();
            client.push_str(&docs::line_comment("    ", "#", &format!("{waiter_docs}Calls {{#{name}}} repeatedly until the modeled `{waiter}` waiter succeeds, with jittered backoff between `min_delay` and `max_delay`. Returns nil when an error acceptor succeeds.

@param timeout [Numeric] the most time to wait, in seconds
@param min_delay [Numeric, nil] the shortest delay between attempts; the modeled value when nil
@param max_delay [Numeric, nil] the longest delay between attempts; the modeled value when nil
@param cancellation [Runtime::CancellationToken, nil]
@return [{output_type}, nil]
@raise [Runtime::WaiterError] when a failure acceptor matches
@raise [Runtime::WaiterTimeoutError] when `timeout` passes first")));
            client.push_str(&format!(
                "    def {}({input}timeout:, min_delay: nil, max_delay: nil, cancellation: nil)\n      @runtime.wait({}, {}, {value}, timeout: timeout, min_delay: min_delay, max_delay: max_delay, cancellation: cancellation)\n    end\n",
                waiter_method(waiter),
                literal(op.name()),
                literal(waiter)
            ));
        }
    }
    client.push_str("  end\nend\n");
    files.push(File {
        path: format!("lib/{package}/client.rb"),
        contents: client,
    });
    files.push(File {
        path: format!("lib/{package}.rb"),
        contents: entry,
    });
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}
