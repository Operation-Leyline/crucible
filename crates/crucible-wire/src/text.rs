//! Text on the wire: query strings, header lists, percent-encoding.

use std::borrow::Cow;

/// The decoded values of query parameter `param`, in order, borrowed from
/// `query` unless they need decoding. `+` is a space.
/// Invalid keys and matching values are errors, never normalized identifiers.
pub fn query_values<'q>(
    query: &'q str,
    param: &'q str,
) -> impl Iterator<Item = Result<Cow<'q, str>, String>> {
    query.split('&').filter_map(move |pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if pair.is_empty() {
            return None;
        }
        match percent_decode(key, true) {
            Ok(key) if key == param => Some(percent_decode(value, true)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        }
    })
}

/// Percent-decoding, borrowing when there is nothing to decode; `plus` reads
/// `+` as a space, as query strings do.
/// Rejects malformed escapes and decoded bytes that are not UTF-8.
pub fn percent_decode(text: &str, plus: bool) -> Result<Cow<'_, str>, String> {
    if !text.bytes().any(|b| b == b'%' || (plus && b == b'+')) {
        return Ok(Cow::Borrowed(text));
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' if plus => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .filter(|hex| hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    None => return Err("contains an invalid percent escape".to_owned()),
                }
            }
            b'%' => return Err("contains an incomplete percent escape".to_owned()),
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8(out)
        .map(Cow::Owned)
        .map_err(|_| "must decode to UTF-8 text".to_owned())
}

/// An absent JSON body contains only the four whitespace bytes JSON allows.
/// Vertical tabs and form feeds must reach the JSON parser and be rejected.
pub fn empty_json_body(body: &[u8]) -> bool {
    body.iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

/// A header list: comma-separated, items quoted when they hold a comma or quote.
pub fn split_header(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }
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
    items
}

/// A header list of HTTP dates, written unquoted as REST protocols send
/// them: every second comma separates two dates.
pub fn split_http_dates(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    parts.chunks(2).map(|pair| pair.join(", ")).collect()
}

pub fn join_header(items: impl IntoIterator<Item = String>) -> String {
    items
        .into_iter()
        .map(|item| {
            if item.is_empty() || item.contains([',', '"', '\\']) || item.trim() != item {
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
        let values: Vec<_> = query_values("a=1&b=x%20y&b=z+w&flag", "b")
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(values, ["x y", "z w"]);
        assert!(matches!(
            query_values("a=1", "a").next(),
            Some(Ok(Cow::Borrowed("1")))
        ));
    }

    #[test]
    fn percent_decoding_rejects_invalid_utf8_and_escapes_without_normalizing_identifiers() {
        for wire in [
            "%FF",
            "%FE",
            "%C0%AF",
            "%ED%A0%80",
            "%",
            "%F",
            "%GG",
            "%aZ",
            "%+A",
            "%-1",
        ] {
            assert!(percent_decode(wire, false).is_err(), "{wire}");
        }
        for (wire, expected) in [("%EF%BF%BD", "�"), ("caf%C3%A9", "café"), ("%2f%2B", "/+")] {
            assert_eq!(percent_decode(wire, false).unwrap(), expected);
        }
        assert!(matches!(
            percent_decode("café+plain", false),
            Ok(Cow::Borrowed("café+plain"))
        ));
        assert_eq!(percent_decode("a+b%2Bc", true).unwrap(), "a b+c");
        assert!(query_values("%FF=x", "q").next().unwrap().is_err());
        assert!(query_values("q=%FF", "q").next().unwrap().is_err());
    }

    #[test]
    fn empty_json_body_allows_only_json_whitespace() {
        for body in [b"".as_slice(), b" \t\n\r"] {
            assert!(empty_json_body(body));
        }
        for body in [b"\x0b".as_slice(), b"\x0c", b"{}", "\u{a0}".as_bytes()] {
            assert!(!empty_json_body(body));
        }
    }

    #[test]
    fn http_date_lists_split_on_every_second_comma() {
        assert_eq!(
            split_http_dates("Mon, 16 Dec 2019 23:48:18 GMT, Tue, 17 Dec 2019 23:48:18 GMT"),
            [
                "Mon, 16 Dec 2019 23:48:18 GMT",
                "Tue, 17 Dec 2019 23:48:18 GMT"
            ]
        );
        assert!(split_http_dates(" ").is_empty());
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

    #[test]
    fn shared_header_lists_preserve_empty_items_and_whitespace() {
        #[derive(serde::Deserialize)]
        struct Case {
            wire: String,
            values: Vec<String>,
            encoded: String,
        }
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../../tests/conformance/header-lists.json"))
                .unwrap();
        for case in cases {
            assert_eq!(split_header(&case.wire), case.values, "{}", case.wire);
            let encoded = join_header(case.values.clone());
            assert_eq!(encoded, case.encoded);
            assert_eq!(split_header(&encoded), case.values);
        }
    }
}
