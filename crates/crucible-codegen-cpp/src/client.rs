use crate::{File, Options, models::Layout, names};
use crucible_smithy::ir::{Service, Shape};

pub fn emit(service: &Service, options: &Options, layout: &Layout<'_>) -> Vec<File> {
    let client_name = format!("{}Client", service.id.name());
    let package = &options.package_name;
    let ns = &options.namespace;
    let mut header = format!(
        "#pragma once\n#include \"models.hpp\"\n#include \"runtime/client_runtime.hpp\"\n\nnamespace {ns} {{\n{}class {client_name} {{\n    std::shared_ptr<runtime::ClientRuntime> runtime_;\npublic:\n    /** Creates a client; copies share runtime and connections. */\n    explicit {client_name}(runtime::ClientConfig config = {{}});\n    /** Closes the shared runtime; finish active calls first. */\n    void close() {{ runtime_->close(); }}\n",
        crucible_codegen_common::doxygen(
            &format!(
                "{}\nConstruct with ClientConfig. Concurrent calls require thread-safe callbacks and transport. Copies share connections and close state; close after calls finish.",
                service.docs.as_deref().unwrap_or("HTTP service client.")
            ),
            ""
        )
    );
    let mut source = format!(
        "#include \"{package}/client.hpp\"\n#include \"{package}/schema.hpp\"\n\nnamespace {ns} {{\nnamespace {{\nstd::exception_ptr decode_error([[maybe_unused]] const std::string& target, [[maybe_unused]] const runtime::Json& value, [[maybe_unused]] runtime::ErrorMetadata metadata) {{\n"
    );
    for (id, shape) in &service.shapes {
        if matches!(shape, Shape::Structure(s) if s.error.is_some()) {
            let ty = format!("::{ns}::models::{}", service.name_of(id));
            let target = names::literal(&id.to_string());
            source.push_str(&format!("    if (target == {target}) return std::make_exception_ptr(runtime::Codec<{ty}>::decode(value, schema().context({target}), std::move(metadata)));\n"));
        }
    }
    source.push_str("    return {};\n}\n}\n");
    source.push_str(&format!("{client_name}::{client_name}(runtime::ClientConfig config) : runtime_(std::make_shared<runtime::ClientRuntime>(std::move(config), schema(), decode_error, {}, {})) {{}}\n", names::literal(options.endpoint.as_deref().unwrap_or("")),options.content_sha256));
    for operation in &service.operations {
        let name = names::field(operation.name());
        let wire_name = names::literal(operation.name());
        let output = operation
            .output
            .as_ref()
            .map(|id| layout.ty(&operation.id, id))
            .unwrap_or_else(|| "runtime::Unit".into());
        let input = operation
            .input
            .as_ref()
            .map(|id| format!("const {}& input, ", layout.ty(&operation.id, id)))
            .unwrap_or_default();
        let encoded = operation
            .input
            .as_ref()
            .map(|id| {
                format!(
                    "runtime::Codec<{}>::encode(input, schema().context({}))",
                    layout.ty(&operation.id, id),
                    names::literal(&id.to_string())
                )
            })
            .unwrap_or_else(|| "runtime::Json(runtime::Json::Object{})".into());
        let request_stream = operation
            .request
            .payload
            .as_ref()
            .filter(|payload| payload.streaming)
            .map(|payload| {
                format!(
                    "runtime::request_stream(input.{})",
                    names::field(&payload.member)
                )
            });
        let response_stream = operation
            .response
            .payload
            .as_ref()
            .filter(|payload| payload.streaming)
            .map(|payload| names::field(&payload.member));
        header.push_str(&crucible_codegen_common::doxygen(
            &crucible_codegen_common::documentation::operation(service, operation),
            "    ",
        ));
        header.push_str(&format!(
            "    [[nodiscard]] {output} {name}({input}std::stop_token stop = {{}}) const;\n"
        ));
        source.push_str(&format!("{output} {client_name}::{name}({input}std::stop_token stop) const {{\n    const auto scope = runtime_->scope(stop);\n    scope.check();\n"));
        if request_stream.is_some() || response_stream.is_some() {
            source.push_str(&format!(
                "    {}auto result = runtime_->exchange({wire_name}, {encoded}, scope, {});\n",
                if operation.output.is_some() {
                    ""
                } else {
                    "[[maybe_unused]] "
                },
                request_stream.as_deref().unwrap_or("std::nullopt")
            ));
        } else {
            source.push_str(&format!(
                "    const auto result = runtime_->call({wire_name}, {encoded}, scope);\n"
            ));
        }
        let decoded = if request_stream.is_some() || response_stream.is_some() {
            "result.value"
        } else {
            "result"
        };
        if let Some(id) = &operation.output {
            source.push_str(&format!(
                "    auto output = runtime::Codec<{output}>::decode({decoded}, schema().context({}));\n",
                names::literal(&id.to_string())
            ));
            if let Some(member) = &response_stream {
                source.push_str(&format!(
                    "    runtime::assign_stream(output.{member}, std::move(result.stream));\n"
                ));
            }
            source.push_str("    scope.check();\n    return output;\n}\n");
        } else {
            source.push_str("    scope.check();\n    return {};\n}\n");
        }
        for waiter in operation.waiters.keys() {
            let method = format!("wait_until_{}", names::field(waiter));
            header.push_str(&format!(
                "    /// Polls {} until the {waiter} waiter's acceptors succeed or fail; throws runtime::WaiterTimeoutError after timeout seconds.\n    [[nodiscard]] std::optional<{output}> {method}({input}double timeout, runtime::WaiterOptions options = {{}}, std::stop_token stop = {{}}) const;\n",
                operation.name()
            ));
            let streams = if request_stream.is_some() || response_stream.is_some() {
                format!(
                    ", {}{}",
                    request_stream.as_deref().unwrap_or("std::nullopt"),
                    response_stream
                        .as_ref()
                        .map(|member| format!(", [](auto& output, std::optional<runtime::ByteStream> stream) {{ runtime::assign_stream(output.{member}, std::move(stream)); }}"))
                        .unwrap_or_default()
                )
            } else {
                String::new()
            };
            source.push_str(&format!(
                "std::optional<{output}> {client_name}::{method}({input}double timeout, runtime::WaiterOptions options, std::stop_token stop) const {{\n    return runtime_->wait<{output}>({wire_name}, {}, {encoded}, timeout, options, stop{streams});\n}}\n",
                names::literal(waiter)
            ));
        }
        if operation.paginated.is_some() {
            header.push_str(&format!("    /** Creates a lazy iterator; next returns nullopt when exhausted. */\n    [[nodiscard]] runtime::Pages<{output}> {name}_pages({input}std::stop_token stop = {{}}) const;\n"));
            source.push_str(&format!("runtime::Pages<{output}> {client_name}::{name}_pages({input}std::stop_token stop) const {{\n    return runtime::Pages<{output}>(runtime_, {wire_name}, {encoded}, stop);\n}}\n"));
        }
    }
    header.push_str("};\n}\n");
    source.push_str("}\n");
    vec![
        File {
            path: format!("include/{package}/client.hpp"),
            contents: header,
        },
        File {
            path: "src/client.cpp".into(),
            contents: source,
        },
    ]
}
