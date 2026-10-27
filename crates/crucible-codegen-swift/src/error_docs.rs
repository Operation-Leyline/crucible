use crucible_ir::{
    Diagnostics, ShapeId,
    ir::{Service, Shape},
};

pub(crate) fn check(service: &Service, diagnostics: &mut Diagnostics) {
    if service.error_docs_base.is_none() {
        return;
    }
    for (id, shape) in &service.shapes {
        if let Shape::Structure(shape) = shape
            && shape.error.is_some()
        {
            for member in &shape.members {
                if super::field(&member.name) == "docURL" {
                    diagnostics.push(
                        &id.with_member(&member.name),
                        "member collides with the configured Swift error docURL accessor",
                    );
                }
            }
        }
    }
}

pub(crate) fn accessor(service: &Service, id: &ShapeId) -> String {
    let Some(docs) = service.error_documentation(id) else {
        return String::new();
    };
    let Some(url) = docs.url else {
        return String::new();
    };
    let mut out = String::from(
        "        /// The model's documentation for this error and its known reason.\n        public var docURL: Swift.String? {\n",
    );
    if let Some(reason) = docs.reason_member {
        let member = service.shapes[id]
            .members()
            .into_iter()
            .find(|member| member.name == reason)
            .unwrap();
        let field = super::field(&reason);
        let optional = if member.optional_on_client() { "?" } else { "" };
        out.push_str(&format!(
            "            switch `{field}`{optional}.rawValue {{\n"
        ));
        for reason in docs.reasons {
            if let Some(url) = reason.url {
                out.push_str(&format!(
                    "            case {}: return {}\n",
                    super::literal(&reason.value),
                    super::literal(&url)
                ));
            }
        }
        out.push_str(&format!(
            "            default: return {}\n            }}\n",
            super::literal(&url)
        ));
    } else {
        out.push_str(&format!("            return {}\n", super::literal(&url)));
    }
    out.push_str("        }\n");
    out
}

/// Specialize only configured SDKs; legacy packages retain their exact API/output.
pub(crate) fn runtime(service: &Service, name: &str, source: &str) -> String {
    if service.error_docs_base.is_none() {
        return source.into();
    }
    match name {
        "ClientConfig" => source
            .replace("    public var name: String", "    public var name: String\n    /// The model's code documentation, when this error was classified.\n    public var docURL: String?")
            .replace("public init(name: String = \"UnknownError\",", "public init(name: String = \"UnknownError\", docURL: String? = nil,")
            .replace("        self.name = name;", "        self.docURL = docURL; self.name = name;")
            .replace("    var metadata: ErrorMetadata { get }", "    var metadata: ErrorMetadata { get }\n    /// The model's documentation for this error and its known reason.\n    var docURL: String? { get }")
            .replace("extension ServiceError { public var description:", "extension ServiceError { public var docURL: String? { metadata.docURL }; public var description:"),
        "ClientRuntime" => source
            .replace("        var body = (try? JSONValue(data: response.body).object) ?? [:]", "        let parsedErrorBody = try? JSONValue(data: response.body).object\n        let emptyErrorBody = response.body.allSatisfy { $0 == 32 || $0 == 9 || $0 == 13 || $0 == 10 }\n        let validErrorObject = emptyErrorBody || parsedErrorBody != nil\n        var body = parsedErrorBody ?? [:]")
            .replace("body = [payload.0: try payloadValue(target, bindings: traits, data: response.body)]", "if traits[\"payloadKind\"]?.text == \"document\" && emptyErrorBody {\n                        let member = try codec.member(target, payload.0)\n                        if member[\"required\"]?.flag == true && (member[\"default\"] == nil || member[\"default\"] == .null) { throw WireError(\"Missing required payload\") }\n                        body = [:]\n                    } else { body = [payload.0: try payloadValue(target, bindings: traits, data: response.body)] }")
            .replace("else { body = try codec.document(target, .object(body), encode: false).object }", "else {\n                    if !validErrorObject { throw WireError(\"Invalid error object\") }\n                    let headerMembers = try traits[\"headers\"]!.array.map { try $0.array[0].string }\n                    for (name, member) in try codec.shape(target)[\"members\"]!.object {\n                        if member[\"required\"]?.flag == true && (member[\"default\"] == nil || member[\"default\"] == .null) && body[name] == nil && !headerMembers.contains(name) {\n                            throw WireError(\"Missing required error body member\")\n                        }\n                    }\n                    body = try codec.document(target, .object(body), encode: false).object\n                }")
            .replace("let metadata = ErrorMetadata(name: name ?? shape?[\"name\"]?.text ?? \"UnknownError\",", "let metadata = ErrorMetadata(name: name ?? shape?[\"name\"]?.text ?? \"UnknownError\", docURL: traits?[\"documentation\"]?[\"url\"]?.text,"),
        _ => source.into(),
    }
}
