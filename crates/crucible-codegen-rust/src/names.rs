//! Spelling model things in Rust.

/// `nextToken` → `next_token`, `HTTPStatus` → `http_status`, keywords escaped.
pub(crate) fn field(name: &str) -> String {
    escape(&snake(name))
}

/// `CreateCity` → `create_city`.
pub(crate) fn function(name: &str) -> String {
    escape(&snake(name))
}

/// `CLOUDY` / `cloudy` / `partly-cloudy` → `Cloudy` / `PartlyCloudy`.
pub(crate) fn variant(name: &str) -> String {
    let words = words(name);
    let mut out: String = words
        .iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
                }
                None => String::new(),
            }
        })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 'V');
    }
    if out == "Self" {
        out.push('_');
    }
    out
}

pub(crate) fn snake(name: &str) -> String {
    words(name).join("_").to_ascii_lowercase()
}

/// Splits on separators and case boundaries; an acronym is one word.
fn words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        let previous = i.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(i + 1).copied();
        let boundary = c.is_ascii_uppercase()
            && previous.is_some_and(|p| {
                p.is_ascii_lowercase()
                    || p.is_ascii_digit()
                    || (p.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase()))
            });
        if boundary && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        word.push(c);
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where",
    "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "try",
    "typeof", "unsized", "virtual", "yield",
];

fn escape(name: &str) -> String {
    match name {
        "self" | "super" | "crate" | "Self" => format!("{name}_"),
        _ if KEYWORDS.contains(&name) => format!("r#{name}"),
        _ if name.starts_with(|c: char| c.is_ascii_digit()) => format!("_{name}"),
        _ => name.to_owned(),
    }
}

pub(crate) fn literal(text: &str) -> String {
    format!("{text:?}")
}

/// `///` docs, or nothing when there are none.
pub(crate) fn doc(out: &mut String, indent: &str, docs: Option<&str>) {
    let Some(docs) = docs.map(str::trim).filter(|d| !d.is_empty()) else {
        return;
    };
    for line in docs.lines() {
        if line.trim().is_empty() {
            out.push_str(&format!("{indent}///\n"));
        } else {
            out.push_str(&format!("{indent}/// {line}\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_snake_case_and_escaped() {
        assert_eq!(field("nextToken"), "next_token");
        assert_eq!(field("HTTPStatus"), "http_status");
        assert_eq!(field("type"), "r#type");
        assert_eq!(field("self"), "self_");
        assert_eq!(field("x-id"), "x_id");
        assert_eq!(field("maxResults"), "max_results");
        assert_eq!(field("v2Name"), "v2_name");
    }

    #[test]
    fn variants_are_upper_camel() {
        assert_eq!(variant("CLOUDY"), "Cloudy");
        assert_eq!(variant("partly-cloudy"), "PartlyCloudy");
        assert_eq!(variant("dataPlane"), "DataPlane");
        assert_eq!(variant("ALL_GRAPHS"), "AllGraphs");
        assert_eq!(variant("2xx"), "V2xx");
    }
}
