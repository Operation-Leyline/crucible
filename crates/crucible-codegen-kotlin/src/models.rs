use super::{field, literal, ty};
use crucible_smithy::ir::{Service, Shape};

pub fn variant(name: &str) -> String {
    let mut chars = name.chars();
    format!(
        "{}{}",
        chars.next().expect("member").to_ascii_uppercase(),
        chars.as_str()
    )
}

pub fn emit(service: &Service, package: &str) -> (String, String) {
    let mut models = String::from("object Models {\n    data object Unit\n");
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
        let name = service.name_of(id);
        let target = literal(&id.to_string());
        let native = format!("Models.`{name}`");
        let encode_start = encode.len();
        let decode_start = decode.len();
        match shape {
            Shape::List(l) => {
                let t = ty(service, &l.member.target);
                models.push_str(&format!(
                    "    class `{name}` : java.util.ArrayList<{t}>()\n"
                ));
                decode.push_str(&format!("        {target} -> {native}().apply {{ addAll(codec.readList(target, value) as List<{t}>) }}\n"));
            }
            Shape::Map(m) => {
                let key = ty(service, &m.key.target);
                let val = ty(service, &m.value.target);
                models.push_str(&format!(
                    "    class `{name}` : java.util.LinkedHashMap<{key}, {val}>()\n"
                ));
                decode.push_str(&format!("        {target} -> {native}().apply {{ putAll(codec.readMap(target, value) as Map<{key}, {val}>) }}\n"));
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
                let prefix = if s.members.is_empty() {
                    "class"
                } else {
                    "data class"
                };
                models.push_str(&format!("    {prefix} `{name}`(\n"));
                for m in &s.members {
                    let t = ty(service, &m.target);
                    let f = field(&m.name);
                    if m.optional_on_client() {
                        models.push_str(&format!("        var `{f}`: {package}.OptionalValue<{t}> = {package}.OptionalValue.unset(),\n"));
                    } else {
                        models.push_str(&format!("        var `{f}`: {t},\n"));
                    }
                }
                models.push_str(&format!(
                    "    ){}\n",
                    if s.error.is_some() {
                        format!(" : {package}.ServiceError()")
                    } else {
                        String::new()
                    }
                ));
                encode.push_str(&format!("        {target} -> com.google.gson.JsonObject().also {{ result ->\n            val model = value as {native}\n"));
                decode.push_str(&format!("        {target} -> {native}(\n"));
                for m in &s.members {
                    let f = field(&m.name);
                    let n = literal(&m.name);
                    let t = ty(service, &m.target);
                    encode.push_str(&format!(
                        "            codec.put(result, target, {n}, model.`{f}`)\n"
                    ));
                    let cast = if m.optional_on_client() {
                        format!("{package}.OptionalValue<{t}>")
                    } else {
                        t
                    };
                    decode.push_str(&format!("            `{f}` = codec.readMember(target, value.asJsonObject, {n}, {}) as {cast},\n",m.optional_on_client()));
                }
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
                    let t = ty(service, &m.target);
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
    models.push_str("}\n");
    encode.push_str("        else -> error(\"No generated encoder for $target\")\n    }\n");
    decode.push_str("        else -> error(\"No generated decoder for $target\")\n    }\n");
    (
        models,
        format!(
            "@Suppress(\"UNCHECKED_CAST\", \"UNUSED_VARIABLE\", \"UNUSED_PARAMETER\")\ninternal object ModelCodecs {{\n{encode}{decode}{helpers}}}\n"
        ),
    )
}
