use super::{code, field, kdoc, kdoc_for, literal, ty};
use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Service, Shape, SimpleKind};
use serde_json::Value;

pub fn variant(name: &str) -> String {
    let mut chars = name.chars();
    format!(
        "{}{}",
        chars.next().expect("member").to_ascii_uppercase(),
        chars.as_str()
    )
}

pub fn emit(service: &Service, package: &str) -> (Vec<(String, String)>, String) {
    let exception = crucible_codegen_common::service_exception_name(service);
    let mut files = Vec::new();
    let mut encode = String::from(
        "    fun encode(target: String, value: Any, codec: JsonCodec): kotlinx.serialization.json.JsonElement = when (target) {\n",
    );
    let mut decode = String::from(
        "    fun decode(target: String, value: kotlinx.serialization.json.JsonElement, codec: JsonCodec): Any = when (target) {\n",
    );
    let mut helpers = String::new();
    for (index, (id, shape)) in service.shapes.iter().enumerate() {
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit { .. }) {
            continue;
        }
        let mut models = String::new();
        let name = service.name_of(id);
        let target = literal(&id.to_string());
        let native = format!("{package}.model.`{name}`");
        let encode_start = encode.len();
        let decode_start = decode.len();
        match shape {
            Shape::List(l) => {
                let t = format!(
                    "{}{}",
                    ty(service, &l.member.target, package),
                    if l.sparse { "?" } else { "" }
                );
                models.push_str(&kdoc_for(
                    "",
                    l.docs.as_deref(),
                    &[format!("A list of {}.", code(&l.member.target.to_string()))],
                    &[],
                    &l.notes,
                    "",
                ));
                if recursive_collection(service, id, id, &mut std::collections::BTreeSet::new()) {
                    models.push_str(&format!(
                        "class `{name}` : kotlin.collections.ArrayList<{t}>() {{\n    override fun toString(): kotlin.String = {}\n}}\n",
                        if service.sensitive(&l.member) { format!("{package}.ModelFormat.REDACTED") } else { "super.toString()".to_string() }
                    ));
                    decode.push_str(&format!("        {target} -> {native}().apply {{ addAll(codec.readList(target, value) as List<{t}>) }}\n"));
                } else {
                    models.push_str(&format!(
                        "typealias `{name}` = kotlin.collections.List<{t}>\n"
                    ));
                    decode.push_str(&format!(
                        "        {target} -> codec.readList(target, value) as {native}\n"
                    ));
                }
            }
            Shape::Map(m) => {
                let key = ty(service, &m.key.target, package);
                let val = format!(
                    "{}{}",
                    ty(service, &m.value.target, package),
                    if m.sparse { "?" } else { "" }
                );
                models.push_str(&kdoc_for(
                    "",
                    m.docs.as_deref(),
                    &["A map shape.".to_string()],
                    &[],
                    &m.notes,
                    "",
                ));
                if recursive_collection(service, id, id, &mut std::collections::BTreeSet::new()) {
                    models.push_str(&format!(
                        "class `{name}` : kotlin.collections.LinkedHashMap<{key}, {val}>() {{\n    override fun toString(): kotlin.String = {}\n}}\n",
                        if service.sensitive(&m.key) || service.sensitive(&m.value) { format!("{package}.ModelFormat.REDACTED") } else { "super.toString()".to_string() }
                    ));
                    decode.push_str(&format!("        {target} -> {native}().apply {{ putAll(codec.readMap(target, value) as Map<{key}, {val}>) }}\n"));
                } else {
                    models.push_str(&format!(
                        "typealias `{name}` = kotlin.collections.Map<{key}, {val}>\n"
                    ));
                    decode.push_str(&format!(
                        "        {target} -> codec.readMap(target, value) as {native}\n"
                    ));
                }
            }
            Shape::Enum(e) => {
                models.push_str(&kdoc_for("    ", e.docs.as_deref(), &["An open enum: a value this client does not know is kept as is, so compare with the constants or `value`.".to_string()], &["@property value the wire value".to_string()], &e.notes, ""));
                models.push_str(&format!("    data class `{name}`(val value: kotlin.String) {{\n        override fun toString(): kotlin.String = value\n        companion object {{\n"));
                for v in &e.values {
                    models.push_str(&kdoc_for(
                        "            ",
                        v.docs.as_deref(),
                        &[format!("The {} value.", code(&v.value))],
                        &[],
                        &v.notes,
                        "",
                    ));
                    models.push_str(&format!(
                        "            @kotlin.jvm.JvmField val `{}` = `{name}`({})\n",
                        v.name.to_uppercase(),
                        literal(&v.value)
                    ));
                }
                models.push_str("        }\n    }\n");
                encode.push_str(&format!(
                    "        {target} -> kotlinx.serialization.json.JsonPrimitive((value as {native}).value)\n"
                ));
                decode.push_str(&format!(
                    "        {target} -> {native}(codec.string(value))\n"
                ));
            }
            Shape::IntEnum(e) => {
                models.push_str(&kdoc_for("", e.docs.as_deref(), &["An open integer enum: a value this client does not know is kept as is, so compare with the constants or `value`.".to_string()], &["@property value the wire value".to_string()], &e.notes, ""));
                models.push_str(&format!("data class `{name}`(val value: kotlin.Int) {{\n    override fun toString(): kotlin.String = value.toString()\n    companion object {{\n"));
                for v in &e.values {
                    models.push_str(&kdoc_for(
                        "        ",
                        v.docs.as_deref(),
                        &[format!("The {} value.", code(&v.value.to_string()))],
                        &[],
                        &v.notes,
                        "",
                    ));
                    models.push_str(&format!(
                        "        @kotlin.jvm.JvmField val `{}` = `{name}`({})\n",
                        v.name.to_uppercase(),
                        v.value
                    ));
                }
                models.push_str("    }\n}\n");
                encode.push_str(&format!("        {target} -> kotlinx.serialization.json.JsonPrimitive((value as {native}).value)\n"));
                decode.push_str(&format!(
                    "        {target} -> {native}(codec.intValue(value))\n"
                ));
            }
            Shape::Structure(s) => {
                let mut class_notes = vec![];
                if let Some(error) = &s.error {
                    class_notes.push(format!("A modeled error: HTTP {}, a {} fault. It is thrown by operations that list it and extends `{exception}`.", error.status, if matches!(error.fault, crucible_smithy::ir::Fault::Client) { "client" } else { "server" }));
                } else {
                    class_notes.push(format!("Build one with `{name} {{ ... }}` or `copy {{ ... }}`; it is immutable. Smithy shape {}.", code(&id.to_string())));
                }
                models.push_str(&kdoc_for(
                    "",
                    s.docs.as_deref(),
                    &class_notes,
                    &[],
                    &s.notes,
                    "",
                ));
                models.push_str(&format!("class `{name}` internal constructor(\n"));
                let mut format_pairs = String::new();
                if s.error.is_some() && !s.members.iter().any(|m| m.name == "message") {
                    format_pairs.push_str(", \"message\" to message");
                }
                let mut builder = String::from("    class Builder {\n");
                let mut properties = String::new();
                let mut build = format!("        fun build(): {native} = {native}(\n");
                let mut copy =
                    String::from("    fun copy(block: Builder.() -> kotlin.Unit = {}): ");
                copy.push_str(&format!("{native} = Builder().also {{ builder ->\n"));
                encode.push_str(&format!("        {target} -> wireObject {{ result ->\n            val model = value as {native}\n"));
                decode.push_str(&format!("        {target} -> {native}(\n"));
                for (index, m) in s.members.iter().enumerate() {
                    let t = ty(service, &m.target, package);
                    let f = field(&m.name);
                    let n = literal(&m.name);
                    let (storage, cast) = if m.optional_on_client() {
                        let storage = format!("__crucibleField{index}");
                        let cast = format!("{package}.OptionalValue<{t}>");
                        models.push_str(&format!("    internal val {storage}: {cast},\n"));
                        let member_doc = |indent: &str| {
                            kdoc_for(indent, m.docs.as_deref().or(Some(&format!("The `{}` member.", m.name))), &["Optional: `null` when unset or explicitly null; use the matching `has` function to tell the two apart.".to_string()], &[], &m.notes, "")
                        };
                        properties.push_str(&member_doc("    "));
                        properties
                            .push_str(&format!("    val `{f}`: {t}? get() = {storage}.value\n"));
                        properties.push_str(&kdoc(
                            "    ",
                            None,
                            &[format!(
                                "Whether {} was set, even to `null`.",
                                code(&m.name)
                            )],
                            &["@return true when the member is present".to_string()],
                        ));
                        properties.push_str(&format!(
                            "    fun has{}(): kotlin.Boolean = {storage}.present\n",
                            variant(&f)
                        ));
                        builder.push_str(&format!("        internal var {storage}: {cast} = {package}.OptionalValue.unset()\n"));
                        builder.push_str(&member_doc("        "));
                        builder.push_str(&format!("        var `{f}`: {t}?\n            get() = {storage}.value\n            set(value) {{ {storage} = {package}.OptionalValue.of(value) }}\n"));
                        builder.push_str(&kdoc(
                            "        ",
                            None,
                            &[format!(
                                "Clears {} so it is omitted from requests.",
                                code(&m.name)
                            )],
                            &[],
                        ));
                        builder.push_str(&format!("        fun unset{}() {{ {storage} = {package}.OptionalValue.unset() }}\n", variant(&f)));
                        build.push_str(&format!("            {storage} = {storage},\n"));
                        copy.push_str(&format!("        builder.{storage} = {storage}\n"));
                        (storage, cast)
                    } else {
                        models.push_str(&kdoc_for(
                            "    ",
                            m.docs
                                .as_deref()
                                .or(Some(&format!("The `{}` member.", m.name))),
                            &[if m.client_default().is_some() {
                                "Has a default value when left unset.".to_string()
                            } else {
                                "Required.".to_string()
                            }],
                            &[],
                            &m.notes,
                            "property:",
                        ));
                        models.push_str(&format!("    val `{f}`: {t},\n"));
                        let required = format!(
                            "requireNotNull(`{f}`) {{ {} }}",
                            literal(&format!("Missing required member: {}", m.name))
                        );
                        let assigned = if let Some(default) = m.client_default() {
                            let default = default_expr(service, &m.target, default, package)
                                .expect("validated default");
                            let assigned = format!("__crucibleField{index}Assigned");
                            builder.push_str(&format!(
                                "        private var {assigned}: kotlin.Boolean = false\n"
                            ));
                            builder.push_str(&kdoc(
                                "        ",
                                m.docs
                                    .as_deref()
                                    .or(Some(&format!("The `{}` member.", m.name))),
                                &["Has a default value when left unset.".to_string()],
                                &[],
                            ));
                            builder.push_str(&format!("        var `{f}`: {t}? = null\n            set(value) {{ field = value; {assigned} = true }}\n"));
                            format!("if ({assigned}) {required} else {default}")
                        } else {
                            builder.push_str(&kdoc(
                                "        ",
                                m.docs
                                    .as_deref()
                                    .or(Some(&format!("The `{}` member.", m.name))),
                                &["Required: `build()` fails when it is left unset.".to_string()],
                                &[],
                            ));
                            builder.push_str(&format!("        var `{f}`: {t}? = null\n"));
                            required
                        };
                        build.push_str(&format!("            `{f}` = {assigned},\n"));
                        copy.push_str(&format!("        builder.`{f}` = this.`{f}`\n"));
                        (format!("`{f}`"), t)
                    };
                    let shown = if s.sensitive() || crucible_codegen_common::redacted(service, m) {
                        format!("{package}.ModelFormat.redact({storage})")
                    } else {
                        storage.clone()
                    };
                    format_pairs.push_str(&format!(", {n} to {shown}"));
                    encode.push_str(&format!(
                        "            codec.put(result, target, {n}, model.{storage})\n"
                    ));
                    decode.push_str(&format!("            {storage} = codec.readMember(target, value.asJsonObject, {n}, {}) as {cast},\n",m.optional_on_client()));
                }
                models.push_str(&format!(
                    "){} {{\n",
                    if s.error.is_some() {
                        format!(" : {package}.{exception}()")
                    } else {
                        String::new()
                    }
                ));
                models.push_str(&properties);
                // Sensitive members never reach the string; see `ModelFormat`.
                models.push_str(&kdoc("    ", None, &["Formats every set member as `Type(member=value, ...)`; sensitive members print as a redaction marker, never their value.".to_string()], &[]));
                models.push_str(&format!("    override fun toString(): kotlin.String = {package}.ModelFormat.format({}{format_pairs})\n", literal(name)));
                models.push_str("    companion object {\n");
                models.push_str(&format!("        operator fun invoke(block: Builder.() -> kotlin.Unit = {{}}): {native} = Builder().apply(block).build()\n    }}\n"));
                copy.push_str("    }.apply(block).build()");
                if s.error.is_some() {
                    copy.push_str(".also { it.initialize(errorName, message, status, requestId, headers, retryable, throttling) }");
                }
                copy.push('\n');
                models.push_str(&copy);
                build.push_str("        )\n");
                builder.push_str(&build);
                builder.push_str("    }\n");
                models.push_str(&builder);
                models.push_str("}\n");
                encode.push_str("        }\n");
                decode.push_str("        )\n");
            }
            Shape::Union(s) => {
                models.push_str(&kdoc_for("    ", s.docs.as_deref(), &["A union: exactly one variant is set. A variant this client does not know arrives as `UnknownVariant`.".to_string()], &[], &s.notes, ""));
                models.push_str(&format!("    sealed interface `{name}` {{\n"));
                encode.push_str(&format!(
                    "        {target} -> when (val model = value as {native}) {{\n"
                ));
                decode.push_str(&format!(
                    "        {target} -> codec.union({target}, value).let {{ (name, item) -> when (name) {{\n"
                ));
                for m in &s.members {
                    let v = variant(&m.name);
                    let t = ty(service, &m.target, package);
                    let n = literal(&m.name);
                    models.push_str(&kdoc_for(
                        "        ",
                        m.docs
                            .as_deref()
                            .or(Some(&format!("The `{}` variant.", m.name))),
                        &[],
                        &["@property value the variant's value".to_string()],
                        &m.notes,
                        "",
                    ));
                    let shown = if s.sensitive() || crucible_codegen_common::redacted(service, m) {
                        format!("{package}.ModelFormat.redact(value)")
                    } else {
                        "value".to_string()
                    };
                    models.push_str(&format!(
                        "        data class `{v}`(val value: {t}?) : `{name}` {{\n            override fun toString(): kotlin.String = {package}.ModelFormat.format({}, {n} to {shown})\n        }}\n", literal(name)
                    ));
                    encode.push_str(&format!("            is {native}.`{v}` -> wireObject {{ codec.put(it, target, {n}, model.value) }}\n"));
                    let wire = literal(&m.name);
                    decode.push_str(&format!("            {wire} -> {native}.`{v}`(codec.readUnionMember(target, {n}, item) as {t}?)\n"));
                }
                models.push_str(&kdoc("        ", None, &["A variant this client does not recognize, preserved so it round-trips. Its payload is never printed, since it may be sensitive.".to_string()], &["@property name the wire name of the variant".to_string(), "@property value the raw JSON value".to_string()]));
                models.push_str(&format!("        data class UnknownVariant(val name: kotlin.String, val value: kotlinx.serialization.json.JsonElement) : `{name}` {{\n            override fun toString(): kotlin.String = {package}.ModelFormat.format({}, \"unknownVariant\" to name, \"value\" to {package}.ModelFormat.REDACTED)\n        }}\n    }}\n", literal(name)));
                encode.push_str(&format!("            is {native}.UnknownVariant -> codec.unknownUnion(target, model.name, model.value)\n        }}\n"));
                decode.push_str(&format!(
                    "            else -> {native}.UnknownVariant(name, item)\n        }} }}\n"
                ));
            }
            _ => unreachable!(),
        }
        files.push((name.to_string(), models));
        // Per-shape methods keep full-service codecs below the JVM's method-size limit.
        let arm_prefix = format!("        {target} -> ");
        for (dispatch, start, name, value_type, result_type) in [
            (
                &mut encode,
                encode_start,
                "encode",
                "Any",
                "kotlinx.serialization.json.JsonElement",
            ),
            (
                &mut decode,
                decode_start,
                "decode",
                "kotlinx.serialization.json.JsonElement",
                "Any",
            ),
        ] {
            let arm = dispatch.split_off(start);
            if let Some(body) = arm.strip_prefix(&arm_prefix) {
                helpers.push_str(&format!("    private fun {name}{index}(target: String, value: {value_type}, codec: JsonCodec): {result_type} = {body}\n"));
                dispatch.push_str(&format!(
                    "{arm_prefix}{name}{index}(target, value, codec)\n"
                ));
            }
        }
    }
    encode.push_str("        else -> error(\"No generated encoder for $target\")\n    }\n");
    decode.push_str("        else -> error(\"No generated decoder for $target\")\n    }\n");
    (
        files,
        format!(
            "@Suppress(\"UNCHECKED_CAST\", \"UNUSED_VARIABLE\", \"UNUSED_PARAMETER\")\ninternal object ModelCodecs {{\n{encode}{decode}{helpers}}}\n"
        ),
    )
}

/// Render the already-lowered default without losing integral precision.
pub(crate) fn default_expr(
    service: &Service,
    target: &ShapeId,
    value: &Value,
    package: &str,
) -> Option<String> {
    match (service.shape(target)?, value) {
        (Shape::Enum(_), Value::String(text)) => Some(format!(
            "{}({})",
            ty(service, target, package),
            literal(text)
        )),
        (Shape::IntEnum(_), Value::Number(value)) => {
            Some(format!("{}({})", ty(service, target, package), value))
        }
        (Shape::Simple(simple), value) => match (simple.kind, value) {
            (SimpleKind::BigInteger, Value::Number(value)) => Some(format!(
                "com.ionspin.kotlin.bignum.integer.BigInteger.parseString({package}.WireDecimal({}).integerText())",
                literal(&value.to_string())
            )),
            (SimpleKind::BigDecimal, Value::Number(value)) => Some(format!(
                "com.ionspin.kotlin.bignum.decimal.BigDecimal.parseString({})",
                literal(&value.to_string())
            )),
            (SimpleKind::Boolean, Value::Bool(value)) => Some(value.to_string()),
            (SimpleKind::String, Value::String(value)) => Some(literal(value)),
            (
                SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long,
                Value::Number(number),
            ) => {
                let Some(value) = number.as_i64() else {
                    let method = match simple.kind {
                        SimpleKind::Byte => "toByte",
                        SimpleKind::Short => "toShort",
                        SimpleKind::Integer => "toInt",
                        SimpleKind::Long => "toLong",
                        _ => unreachable!(),
                    };
                    return Some(format!(
                        "{package}.WireDecimal({}).longExact().{method}()",
                        literal(&number.to_string())
                    ));
                };
                match simple.kind {
                    SimpleKind::Byte => i8::try_from(value)
                        .ok()
                        .map(|value| format!("({value}).toByte()")),
                    SimpleKind::Short => i16::try_from(value)
                        .ok()
                        .map(|value| format!("({value}).toShort()")),
                    SimpleKind::Integer => i32::try_from(value).ok().map(|value| value.to_string()),
                    SimpleKind::Long if value == i64::MIN => Some("kotlin.Long.MIN_VALUE".into()),
                    SimpleKind::Long => Some(format!("{value}L")),
                    _ => unreachable!(),
                }
            }
            (SimpleKind::Float | SimpleKind::Double, Value::Number(number)) => {
                let mut text = number.to_string();
                if !text.contains(['.', 'e', 'E']) {
                    text.push_str(".0");
                }
                if simple.kind == SimpleKind::Float {
                    text.push('f');
                }
                Some(text)
            }
            (SimpleKind::Document, value) => Some(format!(
                "kotlinx.serialization.json.Json.parseToJsonElement({})",
                literal(&value.to_string())
            )),
            (SimpleKind::Blob, Value::String(value)) => Some(format!(
                "kotlin.io.encoding.Base64.decode({})",
                literal(value)
            )),
            (SimpleKind::Timestamp, Value::String(value)) => {
                Some(format!("kotlin.time.Instant.parse({})", literal(value)))
            }
            (SimpleKind::Timestamp, Value::Number(value)) => Some(format!(
                "{package}.WireDecimal({}).instant()",
                literal(&value.to_string())
            )),
            _ => None,
        },
        (Shape::List(_), Value::Array(value)) if value.is_empty() => Some(
            if recursive_collection(
                service,
                target,
                target,
                &mut std::collections::BTreeSet::new(),
            ) {
                format!("{}()", ty(service, target, package))
            } else {
                "kotlin.collections.emptyList()".into()
            },
        ),
        (Shape::Map(_), Value::Object(value)) if value.is_empty() => Some(
            if recursive_collection(
                service,
                target,
                target,
                &mut std::collections::BTreeSet::new(),
            ) {
                format!("{}()", ty(service, target, package))
            } else {
                "kotlin.collections.emptyMap()".into()
            },
        ),
        _ => None,
    }
}

// Kotlin aliases cannot recurse without a nominal type breaking the cycle.
fn recursive_collection(
    service: &Service,
    start: &crucible_smithy::ShapeId,
    current: &crucible_smithy::ShapeId,
    visited: &mut std::collections::BTreeSet<crucible_smithy::ShapeId>,
) -> bool {
    if !visited.insert(current.clone()) {
        return false;
    }
    let targets = match service.shape(current) {
        Some(Shape::List(list)) => vec![&list.member.target],
        Some(Shape::Map(map)) => vec![&map.key.target, &map.value.target],
        _ => return false,
    };
    targets
        .into_iter()
        .any(|target| target == start || recursive_collection(service, start, target, visited))
}
