//! Text on the wire: query strings, header lists, percent-encoding.

use crate::operation::Scalar;
use serde_json::Value;
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

/// A bound member's value as the text of one label, query value or header
/// item: timestamps in the binding's format, media-type strings in base64.
pub fn scalar_text(value: &Value, scalar: Scalar) -> Result<String, String> {
    use crate::operation::TimestampFormat;
    let format = match scalar {
        Scalar::DateTime => Some(TimestampFormat::DateTime),
        Scalar::EpochSeconds => Some(TimestampFormat::EpochSeconds),
        Scalar::HttpDate => Some(TimestampFormat::HttpDate),
        Scalar::Timestamp { text, .. } => Some(text),
        Scalar::Base64 => {
            use base64::Engine;
            return Ok(base64::engine::general_purpose::STANDARD.encode(crate::split::text(value)));
        }
        _ => None,
    };
    match format {
        Some(format) => {
            let time =
                crate::time::from_json(value).ok_or_else(|| "invalid timestamp".to_owned())?;
            crate::time::format(&time, format)
        }
        None => Ok(crate::split::text(value)),
    }
}

/// A header member's text: a list's items joined as a header list.
pub fn header_text(value: &Value, scalar: Scalar) -> Result<String, String> {
    match (value, scalar) {
        // REST protocols send HTTP dates unquoted despite their commas.
        (Value::Array(items), Scalar::List(item)) if item.http_date() => Ok(items
            .iter()
            .map(|value| scalar_text(value, *item))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")),
        (Value::Array(items), Scalar::List(item)) => Ok(join_header(
            items
                .iter()
                .map(|value| scalar_text(value, *item))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        (value, scalar) => scalar_text(value, scalar),
    }
}

/// A deserializer's message without the request value it quotes. serde
/// writes `invalid type: string "123-45-6789", expected i64`, and that text
/// becomes the client's rejection and the service log's record, where a
/// `@sensitive` member's value must not appear. The deserializer does not
/// know which member is sensitive, so no value is kept: its kind stays.
pub fn without_values(message: &str) -> Cow<'_, str> {
    for prefix in ["invalid type: ", "invalid value: "] {
        // The last separator: a quoted string may contain one of its own,
        // and what follows it is the model's, never the request's.
        if let Some(rest) = message.strip_prefix(prefix)
            && let Some(split) = rest.rfind(", expected ")
        {
            let (unexpected, expected) = rest.split_at(split);
            return Cow::Owned(format!("{prefix}{}{expected}", value_kind(unexpected)));
        }
    }
    // The name is the request's text: a variant it sent, or a field a
    // strict structure does not define. What follows is the model's.
    for (prefix, no_choices) in [
        ("unknown variant `", "`, there are no variants"),
        ("unknown field `", "`, there are no fields"),
    ] {
        // The name ends at the last separator of either kind: the request
        // may spell one of its own, but the model's tail spells neither.
        if let Some(rest) = message.strip_prefix(prefix)
            && let Some(split) = ["`, expected ", no_choices]
                .into_iter()
                .filter_map(|separator| rest.rfind(separator))
                .max()
        {
            let kind = &prefix[..prefix.len() - 2];
            return Cow::Owned(format!("{kind}{}", &rest[split + 1..]));
        }
    }
    Cow::Borrowed(message)
}

/// What serde's `Unexpected` names, without its value: `string "x"` is a
/// `string`. A kind serde does not quote stays as written; anything else
/// (`Unexpected::Other`, free text) is only `a value`.
fn value_kind(unexpected: &str) -> &str {
    const QUOTED: [&str; 5] = [
        "boolean",
        "integer",
        "floating point",
        "character",
        "string",
    ];
    // `number` is serde_json's own, under `arbitrary_precision`; `string`
    // through `break` are ciborium's names for CBOR's kinds.
    const BARE: [&str; 19] = [
        "number",
        "string",
        "bytes",
        "tag",
        "null",
        "undefined",
        "simple",
        "break",
        "byte array",
        "unit value",
        "Option value",
        "newtype struct",
        "sequence",
        "map",
        "enum",
        "unit variant",
        "newtype variant",
        "tuple variant",
        "struct variant",
    ];
    if let Some(kind) = QUOTED.into_iter().find(|kind| {
        unexpected
            .strip_prefix(kind)
            .is_some_and(|rest| rest.starts_with(' '))
    }) {
        kind
    } else if BARE.contains(&unexpected) {
        unexpected
    } else {
        "a value"
    }
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

    #[test]
    fn deserializer_messages_keep_the_kind_of_value_and_drop_the_value() {
        #[derive(Debug, serde::Deserialize)]
        enum Sky {
            #[serde(rename = "clear")]
            Clear,
        }
        #[derive(Debug, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        #[allow(dead_code)]
        struct Strict {
            name: Option<String>,
        }
        #[derive(Debug, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Empty {}
        let secret = "123-45-6789";
        // A string that spells the separator must not move the cut.
        let spoof = format!("{secret}, expected nothing");
        let field = format!("{secret}`, expected `name");
        let cases = [
            (
                serde_json::from_value::<i64>(Value::from(secret)).unwrap_err(),
                "invalid type: string, expected i64",
            ),
            (
                serde_json::from_value::<i64>(Value::from(spoof.as_str())).unwrap_err(),
                "invalid type: string, expected i64",
            ),
            (
                serde_json::from_value::<String>(Value::from(4_242_424_242_u64)).unwrap_err(),
                "invalid type: number, expected a string",
            ),
            (
                serde_json::from_value::<char>(Value::from(secret)).unwrap_err(),
                "invalid value: string, expected a character",
            ),
            (
                serde_json::from_value::<String>(serde_json::json!([secret])).unwrap_err(),
                "invalid type: sequence, expected a string",
            ),
            (
                serde_json::from_value::<Sky>(Value::from(secret)).unwrap_err(),
                "unknown variant, expected `clear`",
            ),
            (
                serde_json::from_value::<Strict>(serde_json::json!({ field.as_str(): 1 }))
                    .unwrap_err(),
                "unknown field, expected `name`",
            ),
            (
                serde_json::from_value::<Empty>(serde_json::json!({ secret: 1 })).unwrap_err(),
                "unknown field, there are no fields",
            ),
            // A structure without fields: the client's own `expected` must
            // not end the name early.
            (
                serde_json::from_value::<Empty>(serde_json::json!({ field.as_str(): 1 }))
                    .unwrap_err(),
                "unknown field, there are no fields",
            ),
            (
                serde_json::from_str::<Empty>(&format!("{{\"{secret}`, expected `x\": 1}}"))
                    .unwrap_err(),
                "unknown field, there are no fields at line 1 column 28",
            ),
        ];
        for (error, expected) in cases {
            let message = error.to_string();
            assert_eq!(without_values(&message), expected, "{message}");
        }
        // The kinds serde quotes, as any deserializer reports them.
        use serde::de::{Error, Unexpected};
        for (unexpected, kind) in [
            (Unexpected::Bool(true), "boolean"),
            (Unexpected::Signed(-123_456_789), "integer"),
            (Unexpected::Unsigned(123_456_789), "integer"),
            (Unexpected::Float(1234.5), "floating point"),
            (Unexpected::Char('7'), "character"),
            (Unexpected::Str(secret), "string"),
            (Unexpected::Other(secret), "a value"),
            (Unexpected::Map, "map"),
        ] {
            let error = serde_json::Error::invalid_type(unexpected, &"a PIN");
            assert_eq!(
                without_values(&error.to_string()),
                format!("invalid type: {kind}, expected a PIN")
            );
            let error = serde_json::Error::invalid_value(unexpected, &"a PIN");
            assert_eq!(
                without_values(&error.to_string()),
                format!("invalid value: {kind}, expected a PIN")
            );
        }
        // Positions after the message survive, and messages quoting nothing are untouched.
        let error = serde_json::from_str::<i64>(&format!("\"{secret}\"")).unwrap_err();
        assert_eq!(
            without_values(&error.to_string()),
            "invalid type: string, expected i64 at line 1 column 13"
        );
        assert!(matches!(
            without_values("missing field `city`"),
            Cow::Borrowed("missing field `city`")
        ));
    }
}
