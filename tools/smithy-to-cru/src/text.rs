//! Small writers for `.cru` text. Indentation is left to the formatter.

use serde_json::Value;

pub fn is_ident(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `///` lines for a Markdown documentation string.
pub fn doc(out: &mut String, text: &str) {
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            out.push_str("///\n");
        } else {
            out.push_str("/// ");
            out.push_str(line);
            out.push('\n');
        }
    }
}

/// A JSON value as a `.cru` value (examples, defaults, metadata arguments).
/// `None` for a value `.cru` has no literal for (`null`).
pub fn value(json: &Value) -> Option<String> {
    Some(match json {
        Value::Null => return None,
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => quote(s),
        Value::Array(items) => {
            let items: Option<Vec<String>> = items.iter().map(value).collect();
            format!("[{}]", items?.join(", "))
        }
        Value::Object(fields) => {
            let mut parts = Vec::new();
            for (key, v) in fields {
                let key = if is_ident(key) {
                    key.clone()
                } else {
                    quote(key)
                };
                parts.push(format!("{key}: {}", value(v)?));
            }
            format!("{{ {} }}", parts.join(", "))
        }
    })
}
