use crucible_ir::ir::Service;

/// Configured SDKs alone expose documentation metadata and accessors.
pub(crate) fn runtime(service: &Service, name: &str, source: &str) -> String {
    if service.error_docs_base.is_none() {
        return source.to_owned();
    }
    match name {
        "types.hpp" => source.replace(
            "    bool throttling = false;",
            "    bool throttling = false;\n    /// Static model-derived documentation URL, absent for unknown errors.\n    std::optional<std::string> doc_url;",
        ).replace(
            "    [[nodiscard]] const ErrorMetadata& metadata() const noexcept { return metadata_; }",
            "    [[nodiscard]] const ErrorMetadata& metadata() const noexcept { return metadata_; }\n    [[nodiscard]] const std::optional<std::string>& doc_url() const noexcept { return metadata_.doc_url; }",
        ),
        "client_runtime.cpp" => source.replace(
            "    try { auto decoded = Json::parse(response.body); (void)decoded.object(); body = std::move(decoded); }",
            "    bool valid_error_object = true;\n    for (unsigned char byte : response.body) {\n        if (byte != 32 && byte != 9 && byte != 13 && byte != 10) { valid_error_object = false; break; }\n    }\n    const bool empty_error_body = valid_error_object;\n    try { auto decoded = Json::parse(response.body); (void)decoded.object(); body = std::move(decoded); valid_error_object = true; }",
        ).replace(
            "            auto modeled = payload ?",
            "            if (!payload && !valid_error_object) throw std::invalid_argument(\"Invalid error object\");\n            auto modeled = payload ?",
        ).replace(
            "auto modeled = payload ? Json(Json::Object{{payload->array()[0].string(), payload_value(*schema_, matches[0], *traits, response.body)}}) : document(*schema_, matches[0], body, false);",
            "const auto* payload_kind = traits->find(\"payloadKind\");\n            auto modeled = payload ? (payload_kind && payload_kind->string() == \"document\" && empty_error_body ? Json(Json::Object{}) : Json(Json::Object{{payload->array()[0].string(), payload_value(*schema_, matches[0], *traits, response.body)}})) : document(*schema_, matches[0], body, false);",
        ).replace(
            "    if (shape) {\n        try {",
            "    const auto* documentation = traits ? traits->find(\"documentation\") : nullptr;\n    if (documentation) metadata.doc_url = documentation->at(\"url\").string();\n    if (shape) {\n        try {",
        ).replace(
            "            const auto error = errors_(matches[0], apply_headers(matches[0], *traits, response, modeled), metadata); if (error) return {metadata, error};",
            include_str!("../runtime/error_docs.inc"),
        ).replace(
            "    return {metadata, std::make_exception_ptr(ServiceException(metadata))};",
            "    if (documentation) metadata.doc_url = documentation->at(\"url\").string();\n    return {metadata, std::make_exception_ptr(ServiceException(metadata))};",
        ),
        _ => source.to_owned(),
    }
}
