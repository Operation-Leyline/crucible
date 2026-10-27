use crucible_ir::ir::Service;

/// Keep the unconfigured runtime and its public result layout unchanged.
pub(crate) fn runtime(service: &Service, name: &str, source: &str) -> String {
    if service.error_docs_base.is_none() {
        return source.to_owned();
    }
    match name {
        "client_runtime.h" => source.replace(
            "    bool retryable, throttling;",
            "    /** Static documentation URL copied into the result arena; absent for unknown errors. */\n    crucible_generated_string doc_url;\n    bool retryable, throttling;",
        ),
        "bindings.c" => source.replace(
            "    if (parsed || !yyjson_is_obj(body))",
            "    bool valid_error_object = true;\n    for (size_t i = 0; i < result->response.body.length; ++i) {\n        const unsigned char byte = result->response.body.data[i];\n        if (byte != 32 && byte != 9 && byte != 13 && byte != 10) { valid_error_object = false; break; }\n    }\n    const bool empty_error_body = valid_error_object;\n    valid_error_object = valid_error_object || (!parsed && yyjson_is_obj(body));\n    if (parsed || !yyjson_is_obj(body))",
        ).replace(
            "            code = read_payload(storage, doc, schema, matched, traits, result->response.body, &value, &ignored);\n            if (!code) code = set(doc, typed, text(yyjson_arr_get((yyjson_val *)payload, 0)), value, &ignored);",
            "            code = OK;\n            if (!(named(get(traits, \"payloadKind\"), \"document\") && empty_error_body)) {\n                code = read_payload(storage, doc, schema, matched, traits, result->response.body, &value, &ignored);\n                if (!code) code = set(doc, typed, text(yyjson_arr_get((yyjson_val *)payload, 0)), value, &ignored);\n            }",
        ).replace(
            "    if (matched) {",
            "    if (matched && (valid_error_object || yyjson_is_arr(get(traits, \"payload\")))) {",
        ).replace(
            "    result->retryable =",
            "    const yyjson_val *documentation = get(traits, \"documentation\");\n    const yyjson_val *doc_url = get(documentation, \"url\");\n    if (yyjson_is_str(doc_url)) { TRY(crucible_generated_arena_copy(storage, text(doc_url), &result->doc_url, issue)); }\n    result->retryable =",
        ).replace(
            "        if (!code) result->error_shape = descriptor;",
            include_str!("../runtime/error_docs.inc"),
        ),
        _ => source.to_owned(),
    }
}
