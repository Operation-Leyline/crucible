pub fn literal(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            c if (c as u32) < 32 || c == '\u{7f}' => {
                quoted.push_str(&format!("\\{:03o}", c as u32))
            }
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    if value.contains('\0') {
        format!("std::string({quoted}, {})", value.len())
    } else {
        quoted
    }
}

pub fn keyword(value: &str) -> bool {
    [
        "alignas",
        "alignof",
        "and",
        "and_eq",
        "asm",
        "atomic_cancel",
        "atomic_commit",
        "atomic_noexcept",
        "auto",
        "bitand",
        "bitor",
        "bool",
        "break",
        "case",
        "catch",
        "char",
        "char8_t",
        "char16_t",
        "char32_t",
        "class",
        "compl",
        "concept",
        "const",
        "consteval",
        "constexpr",
        "constinit",
        "const_cast",
        "continue",
        "co_await",
        "co_return",
        "co_yield",
        "decltype",
        "default",
        "delete",
        "do",
        "double",
        "dynamic_cast",
        "else",
        "enum",
        "explicit",
        "export",
        "extern",
        "false",
        "float",
        "for",
        "friend",
        "goto",
        "if",
        "inline",
        "int",
        "long",
        "mutable",
        "namespace",
        "new",
        "noexcept",
        "not",
        "not_eq",
        "nullptr",
        "operator",
        "or",
        "or_eq",
        "private",
        "protected",
        "public",
        "reflexpr",
        "register",
        "reinterpret_cast",
        "requires",
        "return",
        "short",
        "signed",
        "sizeof",
        "static",
        "static_assert",
        "static_cast",
        "struct",
        "switch",
        "synchronized",
        "template",
        "this",
        "thread_local",
        "throw",
        "true",
        "try",
        "typedef",
        "typeid",
        "typename",
        "union",
        "unsigned",
        "using",
        "virtual",
        "void",
        "volatile",
        "wchar_t",
        "while",
        "xor",
        "xor_eq",
    ]
    .contains(&value)
}
pub fn identifier(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_alphabetic())
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !value.contains("__")
        && !keyword(value)
        && !["NULL", "EOF", "NAN", "INFINITY"].contains(&value)
}
pub fn field(value: &str) -> String {
    let chars: Vec<_> = value.chars().collect();
    let mut result = String::new();
    for (index, c) in chars.iter().enumerate() {
        if index > 0
            && c.is_ascii_uppercase()
            && (chars[index - 1].is_ascii_lowercase()
                || chars[index - 1].is_ascii_digit()
                || chars.get(index + 1).is_some_and(char::is_ascii_lowercase)
                    && chars[index - 1].is_ascii_uppercase())
        {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    if result.starts_with('_') {
        result.insert_str(0, "member");
    }
    while result.contains("__") {
        result = result.replace("__", "_u_");
    }
    if keyword(&result) || ["metadata", "what", "min", "max"].contains(&result.as_str()) {
        result.push_str("_value");
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn literals_preserve_escape_text_and_control_bytes() {
        assert_eq!(super::literal("\\u0001"), "\"\\\\u0001\"");
        assert_eq!(super::literal("\u{1}"), "\"\\001\"");
        assert_eq!(super::literal("\0x"), "std::string(\"\\000x\", 2)");
        assert_eq!(super::literal("é"), "\"é\"");
    }
}
