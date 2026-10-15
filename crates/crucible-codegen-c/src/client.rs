use crate::{File, Options, models, names};
use crucible_smithy::ir::{Service, Shape};

pub fn symbols(service: &Service, prefix: &str) -> Vec<(crucible_smithy::ShapeId, String)> {
    let mut result = Vec::new();
    for op in &service.operations {
        let name = format!("{prefix}_operations_{}", names::field(op.name()));
        for suffix in ["", "_result", "_result_free"] {
            result.push((op.id.clone(), format!("{name}{suffix}")));
        }
        if op.paginated.is_some() {
            for suffix in ["_pages_new", "_pages_next"] {
                result.push((op.id.clone(), format!("{name}{suffix}")));
            }
        }
    }
    result
}
pub fn emit(service: &Service, options: &Options, schema: &str) -> Vec<File> {
    let p = &options.prefix;
    let guard = format!("{}_CLIENT_H", p.to_ascii_uppercase());
    let mut header = format!(
        "#ifndef {guard}\n#define {guard}\n#include \"models.h\"\n#include \"runtime/client_runtime.h\"\n\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n{p}_status {p}_client_new(const {p}_client_config *config, {p}_client **output, {p}_error *error);\n"
    );
    let constructor = format!("{p}_status {p}_client_new");
    let docs = crucible_codegen_common::doxygen(
        service.docs.as_deref().unwrap_or("HTTP service client."),
        "",
    ) + &crucible_codegen_common::doxygen(
        "Construct with client_defaults; free after calls and iterators finish. Concurrent calls require thread-safe callbacks and borrowed user data.",
        "",
    );
    header = header.replacen(&constructor, &(docs + &constructor), 1);
    let mut source = format!(
        "#include \"{}/client.h\"\n\nstatic const {p}_string schema_chunks[] = {{\n",
        options.package_name
    );
    for chunk in schema.chars().collect::<Vec<_>>().chunks(512) {
        source.push_str(&format!(
            "    {},\n",
            names::view(&chunk.iter().collect::<String>())
        ));
    }
    source.push_str(&format!("}};\n{p}_status {p}_client_new(const {p}_client_config *config, {p}_client **output, {p}_error *error) {{\n    const {p}_string endpoint = {};\n    return {p}_runtime_new(config, schema_chunks, sizeof(schema_chunks) / sizeof(schema_chunks[0]), endpoint, output, error);\n}}\n", names::view(options.endpoint.as_deref().unwrap_or(""))));
    for op in &service.operations {
        let name = format!("{p}_operations_{}", names::field(op.name()));
        let output = op
            .output
            .as_ref()
            .map(|id| models::native_type(service, id, p))
            .unwrap_or_else(|| format!("{p}_unit"));
        let input = op
            .input
            .as_ref()
            .map(|id| format!("const {} *input, ", models::native_type(service, id, p)))
            .unwrap_or_default();
        let input_value = if op.input.is_some() { "input" } else { "NULL" };
        let docs = crucible_codegen_common::doxygen_for(
            Some(&crucible_codegen_common::documentation::operation(
                service, op,
            )),
            &op.notes,
            "",
        );
        header.push_str(&format!("\n/** Zero-initialize results. Free before reuse; value borrows base.arena. */\ntypedef struct {{ {p}_result base; const {output} *value; }} {name}_result;\n{docs}{p}_status {name}({p}_client *client, {input}{p}_cancelled_fn cancelled, void *cancel_user, {name}_result *result, {p}_error *error);\nvoid {name}_result_free({name}_result *result);\n"));
        source.push_str(&format!("{p}_status {name}({p}_client *client, {input}{p}_cancelled_fn cancelled, void *cancel_user, {name}_result *result, {p}_error *error) {{\n    const {p}_string operation = {};\n    {p}_status status;\n    if (!result) return {p}_client_call(client, operation, {input_value}, cancelled, cancel_user, NULL, error);\n    result->value = NULL;\n    status = {p}_client_call(client, operation, {input_value}, cancelled, cancel_user, &result->base, error);\n    if (status == {p}_ok) result->value = (const {output} *)result->base.output;\n    return status;\n}}\nvoid {name}_result_free({name}_result *result) {{\n    if (result) {{ {p}_result_free(&result->base); result->value = NULL; }}\n}}\n", names::view(op.name())));
        if op.paginated.is_some() {
            header.push_str(&format!("/** Creates a lazy page iterator borrowing client; free with pages_free. */\n{p}_status {name}_pages_new({p}_client *client, {input}{p}_cancelled_fn cancelled, void *cancel_user, {p}_pages **pages, {p}_error *error);\n/** Fetches one page; false has_page means exhaustion. Free each result. */\n{p}_status {name}_pages_next({p}_pages *pages, bool *has_page, {name}_result *result, {p}_error *error);\n"));
            source.push_str(&format!("{p}_status {name}_pages_new({p}_client *client, {input}{p}_cancelled_fn cancelled, void *cancel_user, {p}_pages **pages, {p}_error *error) {{\n    const {p}_string operation = {};\n    return {p}_pages_new(client, operation, {input_value}, cancelled, cancel_user, pages, error);\n}}\n{p}_status {name}_pages_next({p}_pages *pages, bool *has_page, {name}_result *result, {p}_error *error) {{\n    {p}_status status;\n    if (!result) return {p}_pages_next(pages, has_page, NULL, error);\n    result->value = NULL;\n    status = {p}_pages_next(pages, has_page, &result->base, error);\n    if (status == {p}_ok && has_page && *has_page) result->value = (const {output} *)result->base.output;\n    return status;\n}}\n", names::view(op.name())));
        }
    }
    for (id, shape) in &service.shapes {
        if matches!(shape, Shape::Structure(s) if s.error.is_some()) {
            let native = models::native_type(service, id, p);
            let name = format!("{p}_error_as_{}", names::field(service.name_of(id)));
            header.push_str(&format!(
                "const {native} *{name}(const {p}_result *result);\n"
            ));
            source.push_str(&format!("const {native} *{name}(const {p}_result *result) {{\n    return result && result->error_shape == &{} ? (const {native} *)result->modeled_error : NULL;\n}}\n", models::descriptor(service, id, p)));
        }
    }
    header.push_str("\n#ifdef __cplusplus\n}\n#endif\n#endif\n");
    vec![
        File {
            path: format!("include/{}/client.h", options.package_name),
            contents: header,
        },
        File {
            path: "src/client.c".into(),
            contents: source,
        },
    ]
}
