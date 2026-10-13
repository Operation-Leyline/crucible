//! JSON in one canonical form, for digests of a decoded input: what a page
//! token and an idempotency fingerprint are bound to.

use serde_json::Value;

/// JSON with every object's keys sorted, at any depth. `serde_json` sorts
/// them only while nothing in the build enables its `preserve_order`
/// feature, and a map member's iteration order is not stable either way, so
/// two requests that differ only in the order members arrived in write alike.
pub(crate) fn write_canonical(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Object(members) => {
            let mut sorted: Vec<_> = members.iter().collect();
            sorted.sort_unstable_by_key(|(key, _)| key.as_str());
            out.push(b'{');
            for (index, (key, value)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(&Value::String(key.clone()), out);
                out.push(b':');
                write_canonical(value, out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        scalar => out.extend_from_slice(scalar.to_string().as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::write_canonical;

    fn canonical(value: &serde_json::Value) -> String {
        let mut out = Vec::new();
        write_canonical(value, &mut out);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn escapes_keys_and_strings() {
        assert_eq!(
            canonical(&json!({"b\"": "x\ny", "a": [1, true, null, 2.5]})),
            r#"{"a":[1,true,null,2.5],"b\"":"x\ny"}"#
        );
    }

    #[test]
    fn sorts_keys_at_every_depth_and_keeps_array_order() {
        let one: serde_json::Value =
            serde_json::from_str(r#"{"f":{"x":[{"b":1,"a":2}],"y":"q"},"e":null}"#).unwrap();
        let two: serde_json::Value =
            serde_json::from_str(r#"{"e":null,"f":{"y":"q","x":[{"a":2,"b":1}]}}"#).unwrap();
        assert_eq!(canonical(&one), canonical(&two));
        assert_ne!(canonical(&json!(["a", "b"])), canonical(&json!(["b", "a"])));
    }
}
