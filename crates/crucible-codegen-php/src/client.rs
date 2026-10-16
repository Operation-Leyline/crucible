use crate::{
    Options, names,
    names::{literal, ty},
};
use crucible_codegen_common::docs;
use crucible_ir::ir::{Operation, Service};

fn client_docs(service: &Service, namespace: &str, client_name: &str) -> String {
    let mut text = format!(
        "Client for the {} service.\n\n",
        service
            .title
            .as_deref()
            .unwrap_or_else(|| service.id.name())
    );
    if let Some(service_docs) = &service.docs {
        text.push_str(&crate::links::escape_block(
            service,
            namespace,
            service_docs,
        ));
        text.push_str("\n\n");
    }
    text.push_str(&format!(
        "Create one `{client_name}` per configuration and reuse it: it keeps its HTTP connections open between calls. A client is not safe to share across threads or fibers that run calls at the same time; give each its own. Call `close()` when finished. Every operation blocks until it completes and reports failure as a ServiceException for a service response, a NetworkException when no response arrives, a TimeoutException when the call outlasts `ClientConfig::$timeout`, or a CancelledException."
    ));
    docs::block_comment("", &text, &[])
}

fn operation_docs(
    service: &Service,
    namespace: &str,
    op: &Operation,
    input: Option<&str>,
    output: &str,
    pages: bool,
) -> String {
    let token = op.idempotency_token.as_deref().map(names::field);
    let mut text = String::new();
    if let Some(summary) = &op.docs {
        text.push_str(&crate::links::escape_block(service, namespace, summary));
        text.push_str("\n\n");
    }
    text.push_str(&docs::operation_notes(op, token.as_deref()).join("\n\n"));
    for sentence in docs::requirement_notes(service, op) {
        text.push_str("\n\n");
        text.push_str(&sentence);
    }
    for note in docs::note_paragraphs_block(&op.notes) {
        text.push_str("\n\n");
        text.push_str(&note);
    }
    if pages {
        text.push_str(
            "\n\nThis operation is paginated: use the `Pages` variant to read every page.",
        );
    }
    let mut tags = Vec::new();
    if let Some(deprecated) = &op.notes.deprecated {
        tags.push(format!(
            "@deprecated {}",
            docs::escape_block(&deprecated.note())
        ));
    }
    if let Some(input) = input {
        tags.push(format!("@param {input} $input the request to send"));
    }
    tags.push(
        "@param ?Runtime\\CancellationToken $cancellation cancels the call when cancelled"
            .to_string(),
    );
    tags.push(format!("@return {output} the service's response"));
    for name in docs::operation_errors(service, op) {
        tags.push(format!("@throws \\{namespace}\\{name}"));
    }
    tags.push(format!(
        "@throws \\{namespace}\\{} for any other error response",
        crucible_codegen_common::service_exception_name(service)
    ));
    docs::block_comment("    ", &text, &tags)
}

pub fn emit(service: &Service, options: &Options) -> String {
    let namespace = &options.namespace;
    let client_name = crate::client_name(service);
    let endpoint = literal(options.endpoint.as_deref().unwrap_or(""));
    let mut out = format!(
        "<?php\ndeclare(strict_types=1);\nnamespace {namespace};\n\n{}final class {client_name}\n{{\n    private readonly Runtime\\ClientRuntime $runtime;\n{}    public function __construct(Runtime\\ClientConfig $config = new Runtime\\ClientConfig())\n    {{\n        $this->runtime = new Runtime\\ClientRuntime($config, Schema::codec(), Decoder::error(...), {endpoint});\n    }}\n    /** Releases the HTTP transport this client created. A transport passed in the configuration is left open. */\n    public function close(): void {{ $this->runtime->close(); }}\n",
        client_docs(service, namespace, &client_name),
        docs::block_comment("    ", "Creates a client. Without a `$config`, the generated endpoint is used and no credentials are sent; see ClientConfig for credentials, retries, timeouts and the transport.", &["@throws \\InvalidArgumentException when the endpoint or another setting is invalid".to_string()]),
    );
    for op in &service.operations {
        let name = crate::operation_method(op);
        let operation = literal(op.name());
        let input = op
            .input
            .as_ref()
            .map(|id| format!("{} $input, ", ty(service, id, namespace, false)))
            .unwrap_or_default();
        let argument = if op.input.is_some() {
            "$input"
        } else {
            "new Runtime\\Unit()"
        };
        let output = op
            .output
            .as_ref()
            .map(|id| ty(service, id, namespace, false))
            .unwrap_or("Runtime\\Unit".into());
        let input_type = op
            .input
            .as_ref()
            .map(|id| ty(service, id, namespace, false));
        out.push_str(&operation_docs(
            service,
            namespace,
            op,
            input_type.as_deref(),
            &output,
            op.paginated.is_some(),
        ));
        out.push_str(&format!("    public function {name}({input}?Runtime\\CancellationToken $cancellation = null): {output}\n    {{\n        return $this->runtime->call({operation}, {argument}, $cancellation);\n    }}\n"));
        if op.paginated.is_some() {
            out.push_str(&docs::block_comment("    ", &format!("Every page of `{name}()`, following the pagination token until the service stops returning one. The generator makes one call per page, so each can fail."), &[format!("@return \\Generator<int, {output}> the output of each page")]));
            out.push_str(&format!("    public function {}({input}?Runtime\\CancellationToken $cancellation = null): \\Generator\n    {{\n        return $this->runtime->pages({operation}, {argument}, $cancellation);\n    }}\n", crate::pages_method(&name)));
        }
    }
    out.push_str("}\n");
    out
}
