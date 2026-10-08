pub fn literal(value: &str) -> String {
    let mut result = String::from("\"");
    for byte in value.bytes() {
        match byte {
            b'"' => result.push_str("\\\""),
            b'\\' => result.push_str("\\\\"),
            b'?' => result.push_str("\\077"),
            32..=126 => result.push(char::from(byte)),
            _ => result.push_str(&format!("\\{byte:03o}")),
        }
    }
    result.push('"');
    result
}
pub fn view(value: &str) -> String {
    format!("{{{}, {}}}", literal(value), value.len())
}
pub fn identifier(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_alphabetic())
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !value.contains("__")
        && !keyword(value)
}
pub fn keyword(value: &str) -> bool {
    [
        "auto",
        "break",
        "case",
        "char",
        "const",
        "continue",
        "default",
        "do",
        "double",
        "else",
        "enum",
        "extern",
        "float",
        "for",
        "goto",
        "if",
        "inline",
        "int",
        "long",
        "register",
        "restrict",
        "return",
        "short",
        "signed",
        "sizeof",
        "static",
        "struct",
        "switch",
        "typedef",
        "union",
        "unsigned",
        "void",
        "volatile",
        "while",
        "class",
        "new",
        "this",
        "private",
        "public",
        "namespace",
        "template",
        "typename",
        "using",
        "virtual",
        "operator",
        "constexpr",
        "nullptr",
        "delete",
        "friend",
        "throw",
        "try",
        "catch",
        "explicit",
        "export",
        "mutable",
        "noexcept",
        "override",
        "final",
        "decltype",
        "static_cast",
        "reinterpret_cast",
        "const_cast",
        "dynamic_cast",
        "protected",
        "and",
        "and_eq",
        "asm",
        "bitand",
        "bitor",
        "compl",
        "not",
        "not_eq",
        "or",
        "or_eq",
        "xor",
        "xor_eq",
        "typeid",
        "wchar_t",
        "char8_t",
        "char16_t",
        "char32_t",
        "co_await",
        "co_return",
        "co_yield",
        "concept",
        "requires",
        "consteval",
        "constinit",
        "alignas",
        "alignof",
        "bool",
        "complex",
        "imaginary",
        "noreturn",
        "static_assert",
        "thread_local",
        "true",
        "false",
        "NULL",
        "EOF",
    ]
    .contains(&value)
}
pub fn field(value: &str) -> String {
    let chars: Vec<_> = value.chars().collect();
    let mut out = String::new();
    for (index, c) in chars.iter().enumerate() {
        if index > 0
            && c.is_ascii_uppercase()
            && (chars[index - 1].is_ascii_lowercase()
                || chars[index - 1].is_ascii_digit()
                || chars[index - 1].is_ascii_uppercase()
                    && chars.get(index + 1).is_some_and(char::is_ascii_lowercase))
        {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    if out.starts_with('_') {
        out.insert_str(0, "member");
    }
    while out.contains("__") {
        out = out.replace("__", "_u_");
    }
    if keyword(&out) || ["errno", "stdin", "stdout", "stderr"].contains(&out.as_str()) {
        out.push_str("_value");
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn escapes_cpp_keywords_in_public_headers() {
        for name in [
            "class",
            "new",
            "this",
            "public",
            "protected",
            "and",
            "or",
            "typeid",
        ] {
            assert_eq!(super::field(name), format!("{name}_value"));
        }
    }
}
