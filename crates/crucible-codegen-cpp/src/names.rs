pub fn literal(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            c if (c as u32) < 32 || c == '\u{7f}' => {
                quoted.push_str(&format!("\\{:03o}", c as u32))
            }
            // C1 controls, line separators, a BOM and bidirectional controls
            // (which -Wbidi-chars refuses): their UTF-8 bytes, so the literal
            // stays the same bytes and visible in a diff.
            c if crucible_codegen_common::bidi(c)
                || c.is_control()
                || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}') =>
            {
                let mut bytes = [0; 4];
                for byte in c.encode_utf8(&mut bytes).bytes() {
                    quoted.push_str(&format!("\\{byte:03o}"));
                }
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
    if keyword(&result)
        || [
            "metadata", "what", "min", "max", "errno", "stdin", "stdout", "stderr",
        ]
        .contains(&result.as_str())
    {
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

    #[test]
    fn detects_cpp_keywords() {
        assert!(super::keyword("alignas"));
        assert!(super::keyword("bool"));
        assert!(super::keyword("class"));
        assert!(super::keyword("return"));
        assert!(super::keyword("while"));

        assert!(!super::keyword("align"));
        assert!(!super::keyword("Bool"));
        assert!(!super::keyword("CLASS"));
        assert!(!super::keyword("my_variable"));
        assert!(!super::keyword("hello"));
    }

    #[test]
    fn escapes_macro_like_member_names() {
        for name in ["errno", "stdin", "stdout", "stderr"] {
            assert_eq!(super::field(name), format!("{name}_value"));
        }
    }

    #[test]
    fn validates_identifiers() {
        assert!(super::identifier("foo"));
        assert!(super::identifier("foo_bar"));
        assert!(super::identifier("foo123"));
        assert!(super::identifier("XMLHTTPRequest"));

        assert!(!super::identifier("123foo"));
        assert!(!super::identifier("_foo")); // value.starts_with(|c: char| c.is_ascii_alphabetic())
        assert!(!super::identifier("foo-bar"));
        assert!(!super::identifier("foo bar"));
        assert!(!super::identifier("foo__bar"));
        assert!(!super::identifier("class")); // keyword
        assert!(!super::identifier("NULL")); // reserved
        assert!(!super::identifier(""));
    }

    #[test]
    fn sanitizes_field_names() {
        assert_eq!(super::field("fooBar"), "foo_bar");
        assert_eq!(super::field("FooBar"), "foo_bar");
        assert_eq!(super::field("XMLHTTPRequest"), "xmlhttp_request");
        assert_eq!(super::field("foo123Bar"), "foo123_bar");
        assert_eq!(super::field("class"), "class_value");
        assert_eq!(super::field("_unknown"), "member_unknown");
        assert_eq!(super::field("__foo__"), "member_u_foo_u_");
        assert_eq!(super::field("metadata"), "metadata_value");
    }
}
