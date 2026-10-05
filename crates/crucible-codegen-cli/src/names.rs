//! Command spelling uses the same word boundaries as the Rust SDK.

pub(crate) fn snake(name: &str) -> String {
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
    words.join("_").to_ascii_lowercase()
}

pub(crate) fn kebab(name: &str) -> String {
    snake(name).replace('_', "-")
}

pub(crate) fn function(name: &str) -> String {
    let name = snake(name);
    match name.as_str() {
        "self" | "super" | "crate" => format!("{name}_"),
        _ if name.starts_with(|c: char| c.is_ascii_digit()) => format!("_{name}"),
        // Raw identifiers also work for ordinary non-keyword names.
        _ => format!("r#{name}"),
    }
}

pub(crate) fn client(name: &str) -> String {
    let mut name: String = snake(name)
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or(String::new(), |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        name.insert(0, 'V');
    }
    if name == "Self" {
        name.push('_');
    }
    format!("{name}Client")
}
