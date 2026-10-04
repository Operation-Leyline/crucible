//! Text on the wire: query strings, header lists, percent-encoding.

use std::borrow::Cow;

/// The decoded values of query parameter `param`, in order, borrowed from
/// `query` unless they need decoding. `+` is a space.
pub fn query_values<'q>(query: &'q str, param: &'q str) -> impl Iterator<Item = Cow<'q, str>> {
    query.split('&').filter_map(move |pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (!pair.is_empty() && percent_decode(key, true) == param)
            .then(|| percent_decode(value, true))
    })
}

/// Percent-decoding, borrowing when there is nothing to decode; `plus` reads
/// `+` as a space, as query strings do.
pub fn percent_decode(text: &str, plus: bool) -> Cow<'_, str> {
    if !text.bytes().any(|b| b == b'%' || (plus && b == b'+')) {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' if plus => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    Cow::Owned(String::from_utf8_lossy(&out).into_owned())
}

/// A header list: comma-separated, items quoted when they hold a comma or quote.
pub fn split_header(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let mut item = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => item.extend(chars.next()),
                    '"' => break,
                    c => item.push(c),
                }
            }
            while chars.peek().is_some_and(|c| *c != ',') {
                chars.next();
            }
        } else {
            while let Some(c) = chars.peek().copied().filter(|c| *c != ',') {
                item.push(c);
                chars.next();
            }
            item = item.trim_end().to_owned();
        }
        items.push(item);
        if chars.next().is_none() {
            break;
        }
    }
    if items.len() == 1 && items[0].is_empty() {
        items.clear();
    }
    items
}

pub fn join_header(items: impl IntoIterator<Item = String>) -> String {
    items
        .into_iter()
        .map(|item| {
            if item.contains([',', '"']) {
                format!("\"{}\"", item.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                item
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_decode_only_what_they_must() {
        let values: Vec<_> = query_values("a=1&b=x%20y&b=z+w&flag", "b").collect();
        assert_eq!(values, ["x y", "z w"]);
        assert!(matches!(
            query_values("a=1", "a").next(),
            Some(Cow::Borrowed("1"))
        ));
    }

    #[test]
    fn header_lists_round_trip_quoted_items() {
        let items = vec![
            "plain".to_owned(),
            "a,b".to_owned(),
            "say \"hi\"".to_owned(),
        ];
        assert_eq!(split_header(&join_header(items.clone())), items);
        assert!(split_header("").is_empty());
    }
}
