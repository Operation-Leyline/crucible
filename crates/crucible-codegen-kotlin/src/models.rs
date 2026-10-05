use super::{field, literal, ty};
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
    let mut files = Vec::new();
    let mut encode = String::from(
        "    fun encode(target: String, value: Any, codec: JsonCodec): com.google.gson.JsonElement = when (target) {\n",
    );
    let mut decode = String::from(
        "    fun decode(target: String, value: com.google.gson.JsonElement, codec: JsonCodec): Any = when (target) {\n",
    );
    let mut helpers = String::new();
    for (index, (id, shape)) in service.shapes.iter().enumerate() {
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit) {
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
                let t = ty(service, &l.member.target, package);
                if recursive_collection(service, id, id, &mut std::collections::BTreeSet::new()) {
                    models.push_str(&format!("class `{name}` : java.util.ArrayList<{t}>()\n"));
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
                let val = ty(service, &m.value.target, package);
                if recursive_collection(service, id, id, &mut std::collections::BTreeSet::new()) {
                    models.push_str(&format!(
                        "class `{name}` : java.util.LinkedHashMap<{key}, {val}>()\n"
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
                models.push_str(&format!("    data class `{name}`(val value: kotlin.String) {{\n        override fun toString(): kotlin.String = value\n        companion object {{\n"));
                for v in &e.values {
                    models.push_str(&format!(
                        "            @JvmField val `{}` = `{name}`({})\n",
                        v.name.to_uppercase(),
                        literal(&v.value)
                    ));
                }
                models.push_str("        }\n    }\n");
                encode.push_str(&format!(
                    "        {target} -> com.google.gson.JsonPrimitive((value as {native}).value)\n"
                ));
                decode.push_str(&format!(
                    "        {target} -> {native}(codec.string(value))\n"
                ));
            }
            Shape::Structure(s) => {
                models.push_str(&format!("class `{name}` internal constructor(\n"));
                let mut builder = String::from("    class Builder {\n");
                let mut properties = String::new();
                let mut build = format!("        fun build(): {native} = {native}(\n");
                let mut copy =
                    String::from("    fun copy(block: Builder.() -> kotlin.Unit = {}): ");
                copy.push_str(&format!("{native} = Builder().also {{ builder ->\n"));
                encode.push_str(&format!("        {target} -> com.google.gson.JsonObject().also {{ result ->\n            val model = value as {native}\n"));
                decode.push_str(&format!("        {target} -> {native}(\n"));
                for (index, m) in s.members.iter().enumerate() {
                    let t = ty(service, &m.target, package);
                    let f = field(&m.name);
                    let n = literal(&m.name);
                    let (storage, cast) = if m.optional_on_client() {
                        let storage = format!("__crucibleField{index}");
                        let cast = format!("{package}.OptionalValue<{t}>");
                        models.push_str(&format!("    internal val {storage}: {cast},\n"));
                        properties.push_str(&format!("    val `{f}`: {t}? get() = {storage}.value\n    fun has{}(): kotlin.Boolean = {storage}.present\n", variant(&f)));
                        builder.push_str(&format!("        internal var {storage}: {cast} = {package}.OptionalValue.unset()\n        var `{f}`: {t}?\n            get() = {storage}.value\n            set(value) {{ {storage} = {package}.OptionalValue.of(value) }}\n        fun unset{}() {{ {storage} = {package}.OptionalValue.unset() }}\n", variant(&f)));
                        build.push_str(&format!("            {storage} = {storage},\n"));
                        copy.push_str(&format!("        builder.{storage} = {storage}\n"));
                        (storage, cast)
                    } else {
                        models.push_str(&format!("    val `{f}`: {t},\n"));
                        let required = format!(
                            "requireNotNull(`{f}`) {{ {} }}",
                            literal(&format!("Missing required member: {}", m.name))
                        );
                        let assigned = if let Some(default) = &m.default {
                            let default = default_expr(service, &m.target, default, package)
                                .expect("validated default");
                            let assigned = format!("__crucibleField{index}Assigned");
                            builder.push_str(&format!("        private var {assigned}: kotlin.Boolean = false\n        var `{f}`: {t}? = null\n            set(value) {{ field = value; {assigned} = true }}\n"));
                            format!("if ({assigned}) {required} else {default}")
                        } else {
                            builder.push_str(&format!("        var `{f}`: {t}? = null\n"));
                            required
                        };
                        build.push_str(&format!("            `{f}` = {assigned},\n"));
                        copy.push_str(&format!("        builder.`{f}` = `{f}`\n"));
                        (format!("`{f}`"), t)
                    };
                    encode.push_str(&format!(
                        "            codec.put(result, target, {n}, model.{storage})\n"
                    ));
                    decode.push_str(&format!("            {storage} = codec.readMember(target, value.asJsonObject, {n}, {}) as {cast},\n",m.optional_on_client()));
                }
                models.push_str(&format!(
                    "){} {{\n",
                    if s.error.is_some() {
                        format!(" : {package}.ServiceError()")
                    } else {
                        String::new()
                    }
                ));
                models.push_str(&properties);
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
                models.push_str(&format!("    sealed interface `{name}` {{\n"));
                encode.push_str(&format!(
                    "        {target} -> when (val model = value as {native}) {{\n"
                ));
                decode.push_str(&format!(
                    "        {target} -> codec.union(value).let {{ (name, item) -> when (name) {{\n"
                ));
                for m in &s.members {
                    let v = variant(&m.name);
                    let t = ty(service, &m.target, package);
                    let n = literal(&m.name);
                    models.push_str(&format!(
                        "        data class `{v}`(val value: {t}?) : `{name}`\n"
                    ));
                    encode.push_str(&format!("            is {native}.`{v}` -> com.google.gson.JsonObject().also {{ codec.put(it, target, {n}, model.value) }}\n"));
                    decode.push_str(&format!("            {n} -> {native}.`{v}`(codec.readUnionMember(target, name, item) as {t}?)\n"));
                }
                models.push_str(&format!("        data class UnknownVariant(val name: kotlin.String, val value: com.google.gson.JsonElement) : `{name}`\n    }}\n"));
                encode.push_str(&format!("            is {native}.UnknownVariant -> codec.unknownUnion(target, model.name, model.value)\n        }}\n"));
                decode.push_str(&format!("            else -> {native}.UnknownVariant(name, item.deepCopy())\n        }} }}\n"));
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
                "com.google.gson.JsonElement",
            ),
            (
                &mut decode,
                decode_start,
                "decode",
                "com.google.gson.JsonElement",
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
        (Shape::Simple(simple), value) => match (simple.kind, value) {
            (SimpleKind::Boolean, Value::Bool(value)) => Some(value.to_string()),
            (SimpleKind::String, Value::String(value)) => Some(literal(value)),
            (
                SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long,
                Value::Number(number),
            ) => {
                let Some(value) = number.as_i64() else {
                    let method = match simple.kind {
                        SimpleKind::Byte => "byteValueExact",
                        SimpleKind::Short => "shortValueExact",
                        SimpleKind::Integer => "intValueExact",
                        SimpleKind::Long => "longValueExact",
                        _ => unreachable!(),
                    };
                    return Some(format!(
                        "java.math.BigDecimal({}).{method}()",
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
                "com.google.gson.JsonParser.parseString({})",
                literal(&value.to_string())
            )),
            (SimpleKind::Blob, Value::String(value)) => Some(format!(
                "java.util.Base64.getDecoder().decode({})",
                literal(value)
            )),
            (SimpleKind::Timestamp, Value::String(value)) => {
                Some(format!("java.time.Instant.parse({})", literal(value)))
            }
            (SimpleKind::Timestamp, Value::Number(value)) => Some(format!(
                "java.math.BigDecimal({}).let {{ value -> java.time.Instant.ofEpochSecond(value.toLong(), value.remainder(java.math.BigDecimal.ONE).movePointRight(9).longValueExact()) }}",
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
