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
                if super::field(&member.name) == "docUrl" {
                    diagnostics.push(
                        &id.with_member(&member.name),
                        "member collides with the configured Dart error docUrl accessor",
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
        "  /// The model's documentation for this error and its known reason.\n  @core.override\n  core.String get docUrl {\n",
    );
    if let Some(reason) = docs.reason_member {
        let member = service.shapes[id]
            .members()
            .into_iter()
            .find(|member| member.name == reason)
            .unwrap();
        let field = super::field(&reason);
        let optional = if member.optional_on_client() { "?" } else { "" };
        out.push_str(&format!("    switch ({field}{optional}.value) {{\n"));
        for reason in docs.reasons {
            if let Some(url) = reason.url {
                out.push_str(&format!(
                    "      case {}: return {};\n",
                    super::literal(&reason.value),
                    super::literal(&url)
                ));
            }
        }
        out.push_str("    }\n");
    }
    out.push_str(&format!("    return {};\n  }}\n", super::literal(&url)));
    out
}

/// Specialize only configured SDKs; legacy packages retain their exact API/output.
pub(crate) fn runtime(service: &Service, name: &str, source: &str) -> String {
    if service.error_docs_base.is_none() {
        return source.into();
    }
    match name {
        "errors" => source
            .replace("  final String name;", "  final String name;\n\n  /// The model's code documentation, when this error was classified.\n  final String? docUrl;")
            .replace("{this.name = 'UnknownError',", "{this.name = 'UnknownError',\n      this.docUrl,")
            .replace("  final ErrorMetadata metadata;", "  final ErrorMetadata metadata;\n\n  /// The model's documentation for this error and its known reason.\n  String? get docUrl => metadata.docUrl;"),
        "client_runtime" => source
            .replace("    Map<String, Object?> body = {};", "    Map<String, Object?> body = {};\n    final emptyErrorBody = response.body.every((byte) => byte == 32 || byte == 9 || byte == 13 || byte == 10);\n    var validErrorObject = emptyErrorBody;")
            .replace("      body = wireObject(exactJsonDecode(utf8.decode(response.body)));", "      body = wireObject(exactJsonDecode(utf8.decode(response.body)));\n      validErrorObject = true;")
            .replace("body = {wireString(payload[0]): _payloadValue(target, traits, response.body)};", "if (traits['payloadKind'] == 'document' && emptyErrorBody) {\n            final member = wireObject(wireObject(codec.shape(target)['members'])[wireString(payload[0])]);\n            if (member['required'] == true && member['default'] == null) throw const FormatException('Missing required payload');\n            body = {};\n          } else { body = {wireString(payload[0]): _payloadValue(target, traits, response.body)}; }")
            .replace("else { body = wireObject(codec.document(target, body, encode: false)); }", "else {\n          if (!validErrorObject) throw const FormatException('Invalid error object');\n          final headers = wireArray(traits['headers']);\n          for (final entry in wireObject(shape!['members']).entries) {\n            final member = wireObject(entry.value);\n            if (member['required'] == true && member['default'] == null && !body.containsKey(entry.key) &&\n                !headers.any((binding) => wireArray(binding)[0] == entry.key)) throw const FormatException('Missing required error body member');\n          }\n          body = wireObject(codec.document(target, body, encode: false));\n        }")
            .replace("        name: name ?? shape?['name'] as String? ?? 'UnknownError',", "        name: name ?? shape?['name'] as String? ?? 'UnknownError',\n        docUrl: traits?['documentation'] == null ? null : wireObject(traits!['documentation'])['url'] as String?,"),
        _ => source.into(),
    }
}
