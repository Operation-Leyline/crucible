/// A Ruby string literal. Single-quoted when every character can stand as
/// written; otherwise double-quoted with escapes, since Ruby's lexer reads a
/// raw CRLF as LF even inside a single-quoted literal, and line separators,
/// a BOM and bidirectional controls would be invisible in a reviewed diff.
pub fn literal(value: &str) -> String {
    let escaped = |c: char| {
        c.is_control()
            || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}')
            || crucible_codegen_common::bidi(c)
    };
    if !value.chars().any(escaped) {
        return format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"));
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            // `#{`, `#@` and `#$` interpolate in a double-quoted literal.
            '#' => out.push_str("\\#"),
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

pub fn constant(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_uppercase())
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !["BEGIN", "END"].contains(&value)
}
pub fn root_module(value: &str) -> bool {
    constant(value)
        && ![
            "Array",
            "BasicObject",
            "Binding",
            "Class",
            "Complex",
            "Data",
            "Dir",
            "Encoding",
            "Enumerator",
            "Exception",
            "FalseClass",
            "Fiber",
            "File",
            "Float",
            "Hash",
            "IO",
            "Integer",
            "MatchData",
            "Method",
            "Module",
            "Mutex",
            "NilClass",
            "Numeric",
            "Object",
            "Proc",
            "Queue",
            "Random",
            "Range",
            "Rational",
            "Regexp",
            "Ractor",
            "String",
            "Struct",
            "Symbol",
            "Thread",
            "ThreadGroup",
            "Time",
            "TrueClass",
            "UnboundMethod",
            "Date",
            "DateTime",
            "Set",
            "StandardError",
            "ArgumentError",
            "RuntimeError",
            "TypeError",
            "NameError",
            "NoMethodError",
            "LoadError",
            "SyntaxError",
            "IOError",
            "EOFError",
            "SystemCallError",
            "RangeError",
            "ZeroDivisionError",
            "LocalJumpError",
            "RegexpError",
            "ThreadError",
            "NotImplementedError",
            "SecurityError",
            "SystemExit",
            "SignalException",
            "Interrupt",
            "StopIteration",
            "FrozenError",
            "NoMemoryError",
            "SystemStackError",
            "ENV",
            "ARGV",
            "RUBY_VERSION",
            "RUBY_PLATFORM",
            "RUBY_ENGINE",
            "RUBY_DESCRIPTION",
            "Socket",
            "BasicSocket",
            "IPSocket",
            "TCPSocket",
            "TCPServer",
            "UDPSocket",
            "UNIXSocket",
            "UNIXServer",
            "Addrinfo",
            "SocketError",
            "IPAddr",
        ]
        .contains(&value)
}
/// `mailSend` and `HTTPRequest` are `mail_send` and `http_request`.
pub fn snake(value: &str) -> String {
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
    result
}
pub fn field(value: &str) -> String {
    let mut result = snake(value);
    if [
        "alias",
        "and",
        "begin",
        "break",
        "case",
        "class",
        "def",
        "defined",
        "do",
        "else",
        "elsif",
        "end",
        "ensure",
        "false",
        "for",
        "if",
        "in",
        "module",
        "next",
        "nil",
        "not",
        "or",
        "redo",
        "rescue",
        "retry",
        "return",
        "self",
        "super",
        "then",
        "true",
        "undef",
        "unless",
        "until",
        "when",
        "while",
        "yield",
        "__file__",
        "__line__",
        "__encoding__",
        "to_wire",
        "from_wire",
        "initialize",
        "freeze",
        "send",
        "public_send",
        "object_id",
        "method",
        "methods",
        "is_a",
        "respond_to",
        "inspect",
        "to_s",
        "describe",
        "message",
        "backtrace",
        "cause",
        "exception",
        "metadata",
        "set_backtrace",
        "clone",
        "dup",
        "hash",
        "display",
        "tap",
        "then",
        "itself",
        "instance_eval",
        "instance_exec",
    ]
    .contains(&result.as_str())
        || result.starts_with('_')
    {
        result.push_str("_value");
    }
    result
}
pub fn method(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod literal_tests {
    use super::literal;

    #[test]
    fn plain_text_stays_single_quoted() {
        assert_eq!(literal("it's #{x} \\"), "'it\\'s #{x} \\\\'");
    }

    #[test]
    fn line_breaks_and_invisible_characters_are_escaped() {
        assert_eq!(
            literal("a\r\nb\"#{x}\\\u{0}\u{2028}\u{202e}é"),
            "\"a\\r\\nb\\\"\\#{x}\\\\\\u{0}\\u{2028}\\u{202e}é\""
        );
    }
}
