use crucible_smithy::{
    ShapeId,
    ir::{Service, Shape, SimpleKind},
};

/// A PHP string literal for `value`. Single-quoted unless `value` holds a
/// character an editor, `git`'s `text=auto` or a reader could change or not
/// see (a control character such as CR, a line or paragraph separator, a
/// byte-order mark, a bidirectional control): those are written as
/// `\u{...}` escapes in a double-quoted literal, so the committed SDK holds
/// only visible text and the value survives line-ending normalisation.
pub fn literal(value: &str) -> String {
    if !value.chars().any(invisible) {
        return format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"));
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            c if invisible(c) => out.push_str(&format!("\\u{{{:x}}}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A character a literal must not hold raw.
fn invisible(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
                | '\u{feff}'
        )
}
pub fn identifier(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
pub fn reserved(value: &str) -> bool {
    [
        "abstract",
        "and",
        "array",
        "as",
        "bool",
        "break",
        "callable",
        "case",
        "catch",
        "class",
        "clone",
        "const",
        "continue",
        "declare",
        "default",
        "die",
        "do",
        "echo",
        "else",
        "elseif",
        "empty",
        "enddeclare",
        "endfor",
        "endforeach",
        "endif",
        "endswitch",
        "endwhile",
        "enum",
        "eval",
        "exit",
        "extends",
        "false",
        "final",
        "finally",
        "float",
        "fn",
        "for",
        "foreach",
        "function",
        "global",
        "goto",
        "if",
        "implements",
        "include",
        "include_once",
        "instanceof",
        "insteadof",
        "int",
        "interface",
        "isset",
        "iterable",
        "list",
        "match",
        "mixed",
        "namespace",
        "never",
        "new",
        "null",
        "numeric",
        "object",
        "or",
        "parent",
        "print",
        "private",
        "protected",
        "public",
        "readonly",
        "require",
        "require_once",
        "resource",
        "return",
        "self",
        "static",
        "string",
        "switch",
        "throw",
        "trait",
        "true",
        "try",
        "unset",
        "use",
        "var",
        "void",
        "while",
        "xor",
        "yield",
        "__halt_compiler",
        "__class__",
        "__dir__",
        "__file__",
        "__function__",
        "__line__",
        "__method__",
        "__namespace__",
        "__trait__",
        "__property__",
    ]
    .contains(&value.to_ascii_lowercase().as_str())
}
pub fn field(value: &str) -> String {
    if [
        "this", "GLOBALS", "message", "code", "file", "line", "trace", "previous", "metadata",
    ]
    .contains(&value)
    {
        format!("{value}Value")
    } else {
        value.into()
    }
}
pub fn constant(value: &str) -> String {
    if reserved(value) {
        format!("{value}_VALUE")
    } else {
        value.into()
    }
}
/// `mailSend` and `HTTPRequest` are `MAIL_SEND` and `HTTP_REQUEST`, as
/// PSR-12 writes class constants; a reserved word gets [`constant`]'s
/// suffix.
pub fn screaming(value: &str) -> String {
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
        result.push(c.to_ascii_uppercase());
    }
    constant(&result)
}
/// A PHP string literal for model text: [`literal`] when every character
/// can stand as written; otherwise double-quoted with escapes, so a line
/// break survives a checkout that rewrites line endings, and line
/// separators, a BOM and bidirectional controls stay visible in review.
pub fn string(value: &str) -> String {
    let escaped = |c: char| {
        c.is_control()
            || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}')
            || crucible_codegen_common::bidi(c)
    };
    if !value.chars().any(escaped) {
        return literal(value);
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            // `$name` and `{$` interpolate in a double-quoted literal.
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if escaped(c) => out.push_str(&format!("\\u{{{:x}}}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
/// `waitUntilReady` for a waiter named `Ready`, as AWS SDKs name waiter methods.
pub fn waiter(value: &str) -> String {
    let mut chars = value.chars();
    let first = chars.next().map(|c| c.to_ascii_uppercase());
    format!(
        "waitUntil{}{}",
        first.map(String::from).unwrap_or_default(),
        chars.as_str()
    )
}
pub fn method(value: &str) -> String {
    let mut chars = value.chars();
    let result = format!(
        "{}{}",
        chars.next().expect("name").to_ascii_lowercase(),
        chars.as_str()
    );
    if reserved(&result) || result.starts_with("__") {
        format!("{result}Operation")
    } else {
        result
    }
}
pub fn variant(value: &str) -> String {
    let mut chars = value.chars();
    format!(
        "{}{}",
        chars.next().expect("name").to_ascii_uppercase(),
        chars.as_str()
    )
}
pub fn ty(service: &Service, id: &ShapeId, namespace: &str, docs: bool) -> String {
    let runtime = |name| format!("\\{namespace}\\Runtime\\{name}");
    match service.shape(id).expect("shape") {
        Shape::Unit { .. } => runtime("Unit"),
        Shape::Simple(s) => match s.kind {
            SimpleKind::BigInteger => runtime("BigInteger"),
            SimpleKind::BigDecimal => runtime("BigDecimal"),
            // A streaming blob is the whole HTTP body, never held as a string.
            SimpleKind::Blob if s.streaming => runtime("ByteStream"),
            SimpleKind::String | SimpleKind::Blob => "string".into(),
            SimpleKind::Boolean => "bool".into(),
            SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long => {
                "int".into()
            }
            SimpleKind::Float | SimpleKind::Double => "float".into(),
            SimpleKind::Timestamp => runtime("Timestamp"),
            SimpleKind::Document => runtime("JsonValue"),
        },
        Shape::List(list) if docs => format!(
            "list<{}{}>",
            ty(service, &list.member.target, namespace, true),
            if list.sparse { "|null" } else { "" }
        ),
        Shape::Map(map) if docs => format!(
            "array<string|int, {}{}>",
            ty(service, &map.value.target, namespace, true),
            if map.sparse { "|null" } else { "" }
        ),
        Shape::List(_) | Shape::Map(_) => "array".into(),
        _ => format!("\\{namespace}\\{}", service.name_of(id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal() {
        assert_eq!(literal("foo"), "'foo'");
        assert_eq!(literal("foo\\bar"), "'foo\\\\bar'");
        assert_eq!(literal("foo'bar"), "'foo\\'bar'");
        // CR, CRLF, separators, a BOM and bidi controls are escaped in a
        // double-quoted literal, with `$` and `"` escaped there too.
        assert_eq!(literal("a\r\nb"), "\"a\\u{d}\\u{a}b\"");
        assert_eq!(
            literal("\u{feff}x\u{2028}\u{202e}$\"\\'"),
            "\"\\u{feff}x\\u{2028}\\u{202e}\\$\\\"\\\\'\""
        );
    }

    #[test]
    fn test_identifier() {
        assert!(identifier("foo"));
        assert!(identifier("foo_bar"));
        assert!(identifier("_foo"));
        assert!(identifier("_123"));
        assert!(identifier("Foo123"));

        assert!(!identifier("123foo"));
        assert!(!identifier("foo-bar"));
        assert!(!identifier("foo bar"));
        assert!(!identifier(""));
    }

    #[test]
    fn test_reserved() {
        assert!(reserved("class"));
        assert!(reserved("CLASS"));
        assert!(reserved("echo"));
        assert!(reserved("__DIR__"));

        assert!(!reserved("my_var"));
        assert!(!reserved("notReserved"));
    }

    #[test]
    fn test_field() {
        assert_eq!(field("this"), "thisValue");
        assert_eq!(field("GLOBALS"), "GLOBALSValue");
        assert_eq!(field("message"), "messageValue");

        assert_eq!(field("userId"), "userId");
        assert_eq!(field("This"), "This");
    }

    #[test]
    fn test_constant() {
        assert_eq!(constant("class"), "class_VALUE");
        assert_eq!(constant("CLASS"), "CLASS_VALUE");

        assert_eq!(constant("MY_CONSTANT"), "MY_CONSTANT");
    }

    #[test]
    fn test_screaming() {
        assert_eq!(screaming("read"), "READ");
        assert_eq!(screaming("mailSend"), "MAIL_SEND");
        assert_eq!(screaming("HTTPRequest"), "HTTP_REQUEST");
        assert_eq!(screaming("class"), "CLASS_VALUE");
    }

    #[test]
    fn test_string() {
        assert_eq!(string("it's"), "'it\\'s'");
        assert_eq!(string("a\nb $c \"d\" \\"), "\"a\\nb \\$c \\\"d\\\" \\\\\"");
        assert_eq!(string("\u{202e}\r"), "\"\\u{202e}\\r\"");
    }

    #[test]
    fn test_waiter() {
        assert_eq!(waiter("Ready"), "waitUntilReady");
        assert_eq!(waiter("ready"), "waitUntilReady");
        assert_eq!(waiter(""), "waitUntil");
    }

    #[test]
    fn test_method() {
        assert_eq!(method("DescribeInstance"), "describeInstance");
        assert_eq!(method("describeInstance"), "describeInstance");

        assert_eq!(method("Class"), "classOperation");
        assert_eq!(method("Echo"), "echoOperation");

        assert_eq!(method("__init"), "__initOperation");
    }

    #[test]
    fn test_variant() {
        assert_eq!(variant("ready"), "Ready");
        assert_eq!(variant("Ready"), "Ready");
    }
}
