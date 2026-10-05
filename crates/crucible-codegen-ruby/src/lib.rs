//! Ruby gem generation from the shared, lowered Smithy IR.
mod models;
mod names;

use crucible_smithy::{
    Diagnostics,
    ir::{Service, Shape},
};
use names::{constant, field, literal};
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
            Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_)
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
            Shape::Enum(enumeration) => {
                let mut values = BTreeSet::new();
                for value in &enumeration.values {
                    let name = models::enum_constant(&value.name);
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
    }
    diagnostics
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
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
    files.push(File {path:format!("{package}.gemspec"), contents:format!("# frozen_string_literal: true\nGem::Specification.new do |spec|\n  spec.name = {}\n  spec.version = {}\n  spec.summary = {}\n  spec.authors = ['Package maintainers']\n  spec.required_ruby_version = '>= 3.3', '< 5'\n  spec.files = Dir['lib/**/*', 'sig/**/*', 'crucible.json']\n  spec.require_paths = ['lib']\n  spec.add_dependency 'json', '>= 2.7', '< 3'\n  spec.add_dependency 'net-http', '>= 0.4', '< 1'\n  spec.add_dependency 'base64', '>= 0.2', '< 1'\n  spec.add_dependency 'date', '>= 3.3', '< 4'\n  spec.add_dependency 'time', '>= 0.3', '< 1'\n  spec.add_dependency 'securerandom', '>= 0.3', '< 1'\n  spec.add_dependency 'digest', '>= 3.1', '< 4'\nend\n",literal(package),literal(&options.package_version),literal(&format!("Generated {} client",service.name_of(&service.id))))});
    let mut entry = String::from("# frozen_string_literal: true\n");
    for (name, source) in [
        ("unit", include_str!("../runtime/unit.rb")),
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
            contents: source.replace("CrucibleGenerated", module),
        });
    }
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
        "# frozen_string_literal: true\nmodule {module}\n  class {client_name}\n    def initialize(config = Runtime::ClientConfig.new)\n      @runtime = Runtime::ClientRuntime.new(config, ::{module}.codec, endpoint: {}, digest: {})\n    end\n    def close = @runtime.close\n",
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
        client.push_str(&format!("    def {name}({input}cancellation: nil)\n      @runtime.call({}, {value}, cancellation: cancellation)\n    end\n",literal(op.name())));
        if op.paginated.is_some() {
            client.push_str(&format!("    def {name}_pages(input, cancellation: nil)\n      @runtime.pages({}, input, cancellation: cancellation)\n    end\n",literal(op.name())));
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
