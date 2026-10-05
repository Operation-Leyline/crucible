use crate::{
    Options, names,
    names::{literal, method, ty},
};
use crucible_smithy::ir::Service;

pub fn emit(service: &Service, options: &Options) -> String {
    let namespace = &options.namespace;
    let client_name = format!("{}Client", service.id.name());
    let endpoint = literal(options.endpoint.as_deref().unwrap_or(""));
    let digest = if options.content_sha256 {
        "true"
    } else {
        "false"
    };
    let mut out = format!(
        "<?php\ndeclare(strict_types=1);\nnamespace {namespace};\n\nfinal class {client_name}\n{{\n    private readonly Runtime\\ClientRuntime $runtime;\n    public function __construct(Runtime\\ClientConfig $config = new Runtime\\ClientConfig())\n    {{\n        $this->runtime = new Runtime\\ClientRuntime($config, Schema::codec(), Decoder::error(...), {endpoint}, {digest});\n    }}\n    public function close(): void {{ $this->runtime->close(); }}\n"
    );
    for op in &service.operations {
        let name = method(op.name());
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
        if op.response.payload.as_ref().is_some_and(|p| p.streaming) {
            out.push_str(&format!("    /** @param null|\\Closure(string): void|resource $responseTarget receives the streamed body; null buffers it in a temporary stream */\n    public function {name}({input}?Runtime\\CancellationToken $cancellation = null, mixed $responseTarget = null): {output}\n    {{\n        return $this->runtime->call({operation}, {argument}, $cancellation, $responseTarget);\n    }}\n"));
        } else {
            out.push_str(&format!("    public function {name}({input}?Runtime\\CancellationToken $cancellation = null): {output}\n    {{\n        return $this->runtime->call({operation}, {argument}, $cancellation);\n    }}\n"));
        }
        for (waiter, description) in &op.waiters {
            let mut docs = String::new();
            if let Some(text) = &description.documentation {
                for line in text.replace("*/", "*\\/").lines() {
                    docs.push_str(&format!("\n     * {line}"));
                }
            }
            if description.deprecated {
                docs.push_str("\n     * @deprecated");
            }
            out.push_str(&format!(
                "    /**{docs}\n     * Polls {} until the {waiter} waiter succeeds; null when an error acceptor succeeds.\n     * @throws Runtime\\WaiterException when a failure acceptor matches\n     * @throws Runtime\\WaiterTimeoutException when $timeout seconds pass first\n     */\n    public function {}({input}float $timeout, ?float $minDelay = null, ?float $maxDelay = null, ?Runtime\\CancellationToken $cancellation = null): ?{output}\n    {{\n        return $this->runtime->wait({operation}, {}, {argument}, $timeout, $minDelay, $maxDelay, $cancellation);\n    }}\n",
                op.name(),
                names::waiter(waiter),
                literal(waiter),
            ));
        }
        if op.paginated.is_some() {
            out.push_str(&format!("    /** @return \\Generator<int, {output}> */\n    public function {name}Pages({input}?Runtime\\CancellationToken $cancellation = null): \\Generator\n    {{\n        return $this->runtime->pages({operation}, {argument}, $cancellation);\n    }}\n"));
        }
    }
    out.push_str("}\n");
    out
}
