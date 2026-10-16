//! Compact runtime descriptions of already-lowered wire semantics.
pub mod describe;
pub mod doc_links;
pub mod docs;
pub mod documentation;
pub mod json_schema;
pub mod mcp;
pub mod metadata;
pub mod permissions;
pub mod sample;
pub mod surface;

pub use metadata::{PackageMetadata, Registry};

use crucible_ir::ir::{
    Bindings, DocNotes, Fault, Member, Service, Shape, SimpleKind, TimestampFormat,
};
use serde_json::{Value, json};

/// `text` as a double-quoted string literal in a language whose escapes
/// include JSON's (C#, Go, Java, JavaScript, Python, Dart): JSON's quoting,
/// and `\uXXXX` for what JSON leaves raw but a lexer may not accept. C#
/// ends a line at U+0085, U+2028 and U+2029 (not allowed in a string), Go
/// refuses a byte order mark anywhere but a file's start, and the rest
/// (with the [`bidi`] controls) are invisible in a reviewed diff.
pub fn quoted(text: &str) -> String {
    source_json(&serde_json::to_string(text).expect("a string serializes"))
}

/// JSON text as source code can carry it, whole or inside a string literal:
/// what [`quoted`] escapes beyond JSON is escaped here too. Those characters
/// only occur inside JSON strings, so `\uXXXX` keeps the JSON's meaning.
pub fn source_json(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if matches!(
            c,
            '\u{7f}'..='\u{9f}' | '\u{2028}' | '\u{2029}' | '\u{feff}'
        ) || bidi(c)
        {
            out.push_str(&format!("\\u{:04x}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
}

/// The bidirectional formatting characters: the embeddings, overrides and
/// isolates that rustc's deny-by-default `text_direction_codepoint_in_comment`
/// and `text_direction_codepoint_in_literal` lints and GCC/Clang's
/// `-Wbidi-chars` refuse, and the directional marks beside them. Invisible,
/// they make reviewed source read differently from what compiles, so
/// documentation drops them and literals escape them.
pub fn bidi(c: char) -> bool {
    matches!(
        c,
        '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}'
    )
}

/// Public service identity shared by exception classes, protocols and interfaces.
pub fn service_exception_name(service: &Service) -> String {
    format!("{}Exception", service.id.name())
}

/// Specializes the named exception slot in a bundled runtime template.
/// Only call this on runtime source, never on model data or generated literals.
pub fn service_exception_runtime(service: &Service, source: &str) -> String {
    service_exception_runtime_named(&service_exception_name(service), source)
}

/// Uses a target language's spelling of the public service exception name.
pub fn service_exception_runtime_named(name: &str, source: &str) -> String {
    source
        .replace("\r\n", "\n")
        .replace("ServiceException", name)
}

/// Whether a debug representation of `member` must hide its value: the member
/// or its target is `@sensitive`, or it is a collection whose elements are.
/// A collection with any sensitive element is hidden whole, never element by element.
pub fn redacted(service: &Service, member: &Member) -> bool {
    fn nested(service: &Service, member: &Member, seen: &mut Vec<crucible_ir::ShapeId>) -> bool {
        if service.sensitive(member) {
            return true;
        }
        if seen.contains(&member.target) {
            return false;
        }
        seen.push(member.target.clone());
        match service.shape(&member.target) {
            Some(Shape::List(list)) => nested(service, &list.member, seen),
            Some(Shape::Map(map)) => {
                nested(service, &map.key, seen) || nested(service, &map.value, seen)
            }
            _ => false,
        }
    }
    nested(service, member, &mut Vec::new())
}

pub fn check_service_exception_name(service: &Service) -> Result<(), crucible_ir::Diagnostics> {
    let name = service_exception_name(service);
    let mut diagnostics = crucible_ir::Diagnostics::default();
    for id in service.shapes.keys().filter(|id| !id.is_prelude()) {
        if service.name_of(id).eq_ignore_ascii_case(&name) {
            diagnostics.push(
                id,
                format!("model name collides with generated service exception `{name}`"),
            );
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn member(service: &Service, m: &Member, field: &impl Fn(&str) -> String) -> Value {
    let mut value = json!({"target": m.target.to_string(), "attribute": field(&m.name),
        "required": m.required, "default": service.document_wire_default(m), "optional": m.optional_on_client(),
        "format": format(service.document_timestamp_format(m))});
    // Runtimes redact these from telemetry, wherever the member is bound.
    if service.sensitive(m) {
        value["sensitive"] = json!(true);
    }
    value
}

fn format(f: TimestampFormat) -> &'static str {
    match f {
        TimestampFormat::DateTime => "date-time",
        TimestampFormat::HttpDate => "http-date",
        TimestampFormat::EpochSeconds => "epoch-seconds",
    }
}

fn bindings(b: &Bindings) -> Value {
    let mut value = json!({"labels": b.labels, "query": b.query.iter().map(|x| json!([x.member,x.name])).collect::<Vec<_>>(),
        "headers": b.headers.iter().map(|x| json!([x.member,x.name])).collect::<Vec<_>>(), "body": b.body,
        "payload": b.payload.as_ref().map(|p| json!([p.member, p.media_type]))});
    if !b.timestamp_formats.is_empty() {
        value["timestampFormats"] = json!(b.timestamp_formats);
    }
    if !b.header_encodings.is_empty() {
        value["headerEncodings"] = json!(b.header_encodings);
    }
    if let Some(payload) = &b.payload
        && payload.kind != crucible_ir::ir::PayloadKind::Blob
    {
        value["payloadKind"] = json!(payload.kind);
    }
    value
}

pub fn schema(service: &Service, field: impl Fn(&str) -> String) -> Value {
    let shapes: serde_json::Map<String, Value> = service.shapes.iter().map(|(id, shape)| {
        let mut value = match shape {
            Shape::Unit { .. } => json!({"kind":"unit"}),
            Shape::Simple(s) => json!({"kind": match s.kind {
                SimpleKind::String => "string", SimpleKind::Boolean => "boolean",
                SimpleKind::Byte => "byte", SimpleKind::Short => "short", SimpleKind::Integer => "integer", SimpleKind::Long => "long",
                SimpleKind::Float => "float", SimpleKind::Double => "double", SimpleKind::Timestamp => "timestamp", SimpleKind::Document => "document",
                SimpleKind::BigInteger => "bigInteger", SimpleKind::BigDecimal => "bigDecimal",
                SimpleKind::Blob => "blob",
            }, "format": format(s.timestamp_format.unwrap_or_else(|| service.protocol.timestamp_format(false)))}),
            Shape::Enum(e) => json!({"kind":"enum", "values":e.values.iter().map(|v| &v.value).collect::<Vec<_>>()}),
            Shape::IntEnum(e) => json!({"kind":"intEnum", "values":e.values.iter().map(|v| v.value).collect::<Vec<_>>()}),
            Shape::List(l) => {
                let mut value=json!({"kind":"list", "member":member(service,&l.member,&field)});
                if l.sparse {value["sparse"]=json!(true);}
                if l.constraints.unique_items {value["set"]=json!(true);}
                value
            },
            Shape::Map(m) => {
                let mut value=json!({"kind":"map", "key":member(service,&m.key,&field), "value":member(service,&m.value,&field)});
                if m.sparse {value["sparse"]=json!(true);}
                value
            },
            Shape::Structure(s) | Shape::Union(s) => {
                let members: serde_json::Map<_,_> = s.members.iter().map(|m| (m.name.clone(),member(service,m,&field))).collect();
                let mut v = json!({"kind": if matches!(shape,Shape::Union(_)) {"union"} else {"structure"}, "members":members});
                if let Some(e) = &s.error {
                    v["error"] = json!({"status":e.status,"fault":if e.fault == Fault::Client {"client"} else {"server"},
                        "retryable":e.retryable.is_some(),"throttling":e.retryable.is_some_and(|r|r.throttling),
                        "headers":e.headers.iter().map(|b|json!([b.member,b.name])).collect::<Vec<_>>()});
                    if !e.timestamp_formats.is_empty() {v["error"]["timestampFormats"] = json!(e.timestamp_formats);}
                    if e.wire_code != service.name_of(id) {v["error"]["wireCode"] = json!(e.wire_code);}
                    if let Some(payload) = &e.payload {
                        v["error"]["payload"] = json!([payload.member,payload.media_type]);
                        v["error"]["payloadKind"] = json!(payload.kind);
                    }
                    if !e.header_encodings.is_empty() { v["error"]["headerEncodings"] = json!(e.header_encodings); }
                }
                v
            }
        };
        value["name"] = json!(service.name_of(id));
        (id.to_string(),value)
    }).collect();
    let operations: serde_json::Map<String,Value> = service.operations.iter().map(|op| {
        let pagination = op.paginated.as_ref().map(|p|json!({"input":p.input_token,"output":p.output_token.split('.').collect::<Vec<_>>(),"items":p.items,"pageSize":p.page_size}));
        let mut value = json!({"method":op.method,"uri":op.uri.to_string(),"input":op.input.as_ref().map(ToString::to_string),"output":op.output.as_ref().map(ToString::to_string),
            "request":bindings(&op.request),"response":bindings(&op.response),"errors":op.errors.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "safe":op.readonly || op.idempotent || op.idempotency_token.is_some(),"token":op.idempotency_token,"pagination":pagination,
            "bearer":op.auth.iter().any(|id|service.is_bearer(id))});
        if !op.uri.greedy_labels.is_empty() { value["greedyLabels"] = json!(op.uri.greedy_labels); }
        (op.name().into(),value)
    }).collect();
    let mut value = json!({"service":service.id.to_string(),"version":service.version,"shapes":shapes,"operations":operations,
        "bearer":service.auth.iter().any(|id|service.is_bearer(id))});
    // The release every request names in `Crucible-Version`
    // (docs/api-versions-clients.md); absent for a Smithy model.
    if let Some(version) = &service.api_version {
        value["apiVersion"] = json!(version);
    }
    // The permission registry (docs/permissions.md §6.3), for a console's
    // grant picker; never an internal permission, whatever the audience.
    let permissions = permissions::registry(service, false);
    if permissions
        .as_array()
        .is_some_and(|entries| !entries.is_empty())
    {
        value["permissions"] = permissions;
    }
    if !service.recursive_groups.is_empty() {
        value["recursiveGroups"] = json!(service.recursive_groups);
    }
    value
}

#[cfg(test)]
mod quoted_tests {
    use super::quoted;

    #[test]
    fn escapes_bidirectional_controls() {
        let controls: String = [0x202a, 0x202e, 0x2066, 0x2069, 0x200e, 0x200f, 0x061c]
            .into_iter()
            .filter_map(char::from_u32)
            .collect();
        assert_eq!(
            quoted(&format!("{controls}é")),
            r#""\u202a\u202e\u2066\u2069\u200e\u200f\u061cé""#
        );
        let json = format!("{{\"a{}\":\"{}\"}}", '\u{2028}', '\u{202e}');
        assert_eq!(super::source_json(&json), r#"{"a\u2028":"\u202e"}"#);
    }

    #[test]
    fn escapes_what_json_leaves_raw() {
        assert_eq!(
            quoted("a\"\\\r\u{0}\u{7f}\u{85}\u{2028}\u{2029}\u{feff}é"),
            r#""a\"\\\r\u0000\u007f\u0085\u2028\u2029\ufeffé""#
        );
    }
}

#[cfg(test)]
mod tests {
    fn load(name: &str) -> crucible_ir::ir::Service {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/models")
            .join(name);
        crucible_lang::load_service(&dir).unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
    }

    #[test]
    fn service_exception_identity_and_renamed_model_collisions() {
        let mut service = load("weather");
        assert_eq!(super::service_exception_name(&service), "WeatherException");
        assert_eq!(
            super::service_exception_runtime(&service, "class ServiceException : ServiceError {}"),
            "class WeatherException : ServiceError {}"
        );
        service.renames.insert(
            "example.weather#NotFoundError".parse().unwrap(),
            "WeatherException".into(),
        );
        assert!(
            super::check_service_exception_name(&service)
                .unwrap_err()
                .to_string()
                .contains("example.weather#NotFoundError")
        );
    }
    #[test]
    fn unique_items_lists_carry_the_runtime_uniqueness_marker() {
        let service = load("extended-types");
        let schema = super::schema(&service, str::to_owned);
        assert_eq!(schema["shapes"]["example.types#Integers"]["set"], true);
        assert!(schema["shapes"]["example.types#Mode"].get("set").is_none());
    }
    #[test]
    fn sensitive_members_are_described_for_runtimes() {
        let service = load("runtime-policy");
        let schema = super::schema(&service, str::to_owned);
        let members = &schema["shapes"]["example.policy#PutRecordInput"]["members"];
        assert_eq!(members["secret"]["sensitive"], true);
        assert_eq!(members["ticket"]["sensitive"], true);
        assert!(members["label"].get("sensitive").is_none());
    }
    #[test]
    fn renamed_errors_preserve_the_protocol_discriminator() {
        let mut service = load("weather");
        service.renames.insert(
            "example.weather#NotFoundError".parse().unwrap(),
            "MissingResource".into(),
        );
        let schema = super::schema(&service, str::to_owned);
        let shape = &schema["shapes"]["example.weather#NotFoundError"];
        assert_eq!(shape["name"], "MissingResource");
        assert_eq!(shape["error"]["wireCode"], "NotFoundError");
    }

    #[test]
    fn runtime_schema_preserves_effective_client_semantics() {
        let service = load("review-hardening");
        let schema = super::schema(&service, str::to_owned);
        assert_eq!(schema["operations"]["PublicPing"]["bearer"], false);
        assert_eq!(schema["operations"]["ReadValues"]["bearer"], true);
        let member =
            &schema["shapes"]["example.hardening#ReadValuesOutput"]["members"]["optionalDefault"];
        assert_eq!(member["default"], 7);
    }
}

/// CommonMark supported by Doxygen, with comment boundaries and commands escaped.
pub fn doxygen(text: &str, indent: &str) -> String {
    let text = docs::comment_text(text)
        .replace('\\', "&#92;")
        .replace('@', "&#64;")
        .replace("*/", "*&#47;")
        .replace("/*", "/&#42;");
    format!(
        "{indent}/**\n{}{indent} */\n",
        text.lines()
            .map(|line| if line.trim().is_empty() {
                format!("{indent} *\n")
            } else {
                format!("{indent} * {}\n", line.trim_end())
            })
            .collect::<String>()
    )
}

/// [`doxygen`] for model text plus its notes: "Unstable."/"Since X." lines and
/// a `@deprecated` command, which Doxygen renders and IDEs strike through.
/// Empty when there is nothing to say. C has no deprecation attribute in C17,
/// so the command is the whole rendering there.
pub fn doxygen_for(docs: Option<&str>, notes: &DocNotes, indent: &str) -> String {
    let mut paragraphs: Vec<String> = docs.map(str::to_owned).into_iter().collect();
    paragraphs.extend(notes.lines());
    let text = paragraphs.join("\n\n");
    if text.is_empty() && notes.deprecated.is_none() {
        return String::new();
    }
    let mut out = doxygen(&text, indent);
    if let Some(deprecated) = &notes.deprecated {
        let note = docs::comment_text(&deprecated.note())
            .replace('\\', "&#92;")
            .replace('@', "&#64;")
            .replace("*/", "*&#47;")
            .replace("/*", "/&#42;");
        let close = format!("{indent} */\n");
        out.truncate(out.len() - close.len());
        if !text.is_empty() {
            out.push_str(&format!("{indent} *\n"));
        }
        for (index, line) in note.lines().enumerate() {
            let lead = if index == 0 { "@deprecated " } else { "" };
            out.push_str(&format!("{indent} * {lead}{}\n", line.trim_end()));
        }
        out.push_str(&close);
    }
    out
}

/// Native collection aliases cannot customize element formatting. Conservatively
/// redact a collection containing sensitive primitive elements as a whole.
pub fn sensitive_collection(service: &Service, id: &crucible_ir::ShapeId) -> bool {
    fn visit(
        service: &Service,
        id: &crucible_ir::ShapeId,
        seen: &mut std::collections::BTreeSet<crucible_ir::ShapeId>,
    ) -> bool {
        if !seen.insert(id.clone()) {
            return false;
        }
        match service.shape(id) {
            Some(Shape::List(s)) => {
                service.sensitive(&s.member) || visit(service, &s.member.target, seen)
            }
            Some(Shape::Map(s)) => {
                service.sensitive(&s.key)
                    || service.sensitive(&s.value)
                    || visit(service, &s.value.target, seen)
            }
            _ => false,
        }
    }
    visit(service, id, &mut std::collections::BTreeSet::new())
}

#[cfg(test)]
mod documentation_tests {
    #[test]
    fn doxygen_preserves_markdown_without_comment_or_command_injection() {
        let text = super::doxygen(
            "**bold** `code` [link](https://example.invalid)\n/* */ @param \\command",
            "    ",
        );
        assert!(text.contains("**bold** `code` [link](https://example.invalid)"));
        assert!(text.contains("/&#42; *&#47; &#64;param &#92;command"));
        assert_eq!(text.matches("*/").count(), 1);
    }
}
