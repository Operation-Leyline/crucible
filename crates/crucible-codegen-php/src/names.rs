use crucible_smithy::{
    ShapeId,
    ir::{Service, Shape, SimpleKind},
};

pub fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
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
