use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Service, Shape, SimpleKind};

pub fn literal(s: &str) -> String {
    serde_json::to_string(s).expect("a string serializes")
}

pub fn identifier(s: &str) -> bool {
    s.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn keyword(s: &str) -> bool {
    [
        "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
        "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global",
        "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
        "try", "while", "with", "yield",
    ]
    .contains(&s)
}

pub fn field(s: &str) -> String {
    let chars: Vec<_> = s.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i > 0
            && c.is_ascii_uppercase()
            && (chars[i - 1].is_ascii_lowercase()
                || chars[i - 1].is_ascii_digit()
                || chars.get(i + 1).is_some_and(char::is_ascii_lowercase)
                    && chars[i - 1].is_ascii_uppercase())
        {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    if [
        "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
        "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global",
        "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
        "try", "while", "with", "yield", "self", "_unknown", "field",
    ]
    .contains(&out.as_str())
    {
        out.push('_');
    }
    if out.starts_with("__") {
        out.insert_str(0, "member");
    }
    out
}

pub fn enum_member(s: &str) -> String {
    let mut name = field(s).to_uppercase();
    if name.starts_with('_') {
        name.insert_str(0, "VALUE");
    }
    name
}

pub fn ty(service: &Service, id: &ShapeId, prefix: &str) -> String {
    if !id.is_prelude() {
        return format!("{prefix}{}", service.name_of(id));
    }
    match service.shape(id).expect("lowered target") {
        Shape::IntEnum(_) => unreachable!("integer enums are capability-gated"),
        Shape::Unit { .. } => "None".into(),
        Shape::Simple(s) => match s.kind {
            SimpleKind::BigInteger => "int",
            SimpleKind::BigDecimal => "Decimal",
            SimpleKind::String => "str",
            SimpleKind::Boolean => "bool",
            SimpleKind::Float | SimpleKind::Double => "float",
            SimpleKind::Timestamp => "datetime",
            SimpleKind::Document => "Any",
            SimpleKind::Blob => "bytes",
            _ => "int",
        }
        .into(),
        _ => unreachable!("prelude shape"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal() {
        assert_eq!(literal("foo"), "\"foo\"");
        assert_eq!(literal("foo\nbar"), "\"foo\\nbar\"");
        assert_eq!(literal("foo\"bar"), "\"foo\\\"bar\"");
    }

    #[test]
    fn test_identifier() {
        assert!(identifier("foo"));
        assert!(identifier("_foo"));
        assert!(identifier("foo_bar"));
        assert!(identifier("foo123"));
        assert!(!identifier("123foo"));
        assert!(!identifier("foo-bar"));
        assert!(!identifier("foo bar"));
        assert!(!identifier(""));
    }

    #[test]
    fn test_keyword() {
        assert!(keyword("False"));
        assert!(keyword("class"));
        assert!(keyword("yield"));
        assert!(!keyword("foo"));
        assert!(!keyword("Class"));
        assert!(!keyword(""));
    }

    #[test]
    fn test_field() {
        assert_eq!(field("fooBar"), "foo_bar");
        assert_eq!(field("FooBar"), "foo_bar");
        assert_eq!(field("XMLHTTPRequest"), "xmlhttp_request");
        assert_eq!(field("foo123Bar"), "foo123_bar");
        assert_eq!(field("class"), "class_");
        assert_eq!(field("self"), "self_");
        assert_eq!(field("_unknown"), "_unknown_");
        assert_eq!(field("field"), "field_");
    }

    #[test]
    fn test_enum_member() {
        assert_eq!(enum_member("fooBar"), "FOO_BAR");
        assert_eq!(enum_member("FooBar"), "FOO_BAR");
        assert_eq!(enum_member("XMLHTTPRequest"), "XMLHTTP_REQUEST");
        assert_eq!(enum_member("class"), "CLASS_");
        assert_eq!(enum_member("_unknown"), "VALUE_UNKNOWN_");
    }

    #[test]
    fn reserved_member_spellings_remain_accessible() {
        assert_eq!(field("__value"), "member__value");
        assert_eq!(field("__init__"), "member__init__");
        assert_eq!(enum_member("_foo_"), "VALUE_FOO_");
    }
}
