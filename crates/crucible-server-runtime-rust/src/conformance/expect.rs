//! Judging a response against a vector. Every check names what differed,
//! because a conformance failure is read in a test log, not a debugger.

use serde_json::Value;

use super::exchange::Received;
use super::vectors::{BodyAssertion, ExpectedResponse, Refusal};

pub(crate) fn check_response(expected: &ExpectedResponse, got: &Received) -> Result<(), String> {
    if got.status != expected.code {
        return Err(format!(
            "status {} expected, answered {}",
            expected.code,
            got.summary()
        ));
    }
    check_headers(
        expected.headers,
        expected.forbid_headers,
        expected.require_headers,
        got,
    )?;
    if let Some(body) = expected.body {
        check_body(body, expected.body_media_type, got)?;
    }
    Ok(())
}

pub(crate) fn check_refusal(expected: &Refusal, got: &Received) -> Result<(), String> {
    if got.status != expected.code {
        return Err(format!(
            "status {} expected, answered {}",
            expected.code,
            got.summary()
        ));
    }
    check_headers(expected.headers, &[], &[], got)?;
    let Some((media_type, assertion)) = expected.body else {
        return Ok(());
    };
    let answered = got.header("content-type").unwrap_or_default();
    if essence(&answered) != essence(media_type) {
        return Err(format!(
            "body media type {media_type} expected, answered {answered:?}"
        ));
    }
    match assertion {
        BodyAssertion::Contents(contents) => check_body(contents, Some(media_type), got),
        BodyAssertion::MessageRegex(pattern) => {
            let regex = regex::Regex::new(pattern).map_err(|error| {
                format!("messageRegex {pattern:?} is not a Rust regex: {error}")
            })?;
            let message = message_of(got)
                .ok_or_else(|| format!("no error message in the body {:?}", got.body_text()))?;
            if regex.is_match(&message) {
                Ok(())
            } else {
                Err(format!(
                    "the message {message:?} does not match /{pattern}/"
                ))
            }
        }
    }
}

fn check_headers(
    expected: &[(&str, &str)],
    forbidden: &[&str],
    required: &[&str],
    got: &Received,
) -> Result<(), String> {
    for (name, value) in expected {
        match got.header(name) {
            Some(actual) if actual.trim() == value.trim() => {}
            Some(actual) => {
                return Err(format!(
                    "header {name}: {value:?} expected, answered {actual:?}"
                ));
            }
            None => return Err(format!("header {name}: {value:?} expected, none answered")),
        }
    }
    for name in forbidden {
        if got.header(name).is_some() {
            return Err(format!("header {name} must not be answered"));
        }
    }
    for name in required {
        if got.header(name).is_none() {
            return Err(format!("header {name} is required, none answered"));
        }
    }
    Ok(())
}

fn check_body(expected: &str, media_type: Option<&str>, got: &Received) -> Result<(), String> {
    let answered = got.header("content-type");
    let media_type = media_type.or(answered.as_deref());
    if media_type.is_some_and(is_json) && !expected.is_empty() {
        let want: Value = serde_json::from_str(expected)
            .map_err(|error| format!("the expected body is not JSON: {error}"))?;
        let have: Value = serde_json::from_slice(&got.body)
            .map_err(|error| format!("the body is not JSON ({error}): {:?}", got.body_text()))?;
        return if json_equal(&want, &have) {
            Ok(())
        } else {
            Err(format!("body {want} expected, answered {have}"))
        };
    }
    if got.body == expected.as_bytes() {
        Ok(())
    } else {
        Err(format!(
            "body {expected:?} expected, answered {:?}",
            got.body_text()
        ))
    }
}

/// The message a refusal carries: `message` (or `Message`) of a JSON object
/// body, else the whole body.
fn message_of(got: &Received) -> Option<String> {
    let media_type = got.header("content-type");
    if media_type.as_deref().is_some_and(is_json) {
        let Ok(Value::Object(members)) = serde_json::from_slice::<Value>(&got.body) else {
            return None;
        };
        return ["message", "Message"]
            .iter()
            .find_map(|key| members.get(*key))
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    Some(got.body_text())
}

fn essence(media_type: &str) -> String {
    media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn is_json(media_type: &str) -> bool {
    let essence = essence(media_type);
    essence.ends_with("json") || essence.contains("+json")
}

/// JSON equality where `1` and `1.0` are the same number.
pub(crate) fn json_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => {
            a == b || matches!((a.as_f64(), b.as_f64()), (Some(a), Some(b)) if a == b)
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| json_equal(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| json_equal(a, b)))
        }
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn received(status: u16, headers: &[(&str, &str)], body: &str) -> Received {
        Received {
            status,
            headers: headers
                .iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), (*value).to_owned()))
                .collect(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn response(code: u16, body: Option<&'static str>) -> ExpectedResponse {
        ExpectedResponse {
            code,
            headers: &[],
            forbid_headers: &[],
            require_headers: &[],
            body,
            body_media_type: Some("application/json"),
        }
    }

    #[test]
    fn numbers_are_equal_whatever_their_spelling() {
        assert!(json_equal(
            &json!({"a": [1, 2.5]}),
            &json!({"a": [1.0, 2.5]})
        ));
        assert!(!json_equal(&json!({"a": 1}), &json!({"a": 2})));
        assert!(!json_equal(&json!([1]), &json!([1, 2])));
        assert!(!json_equal(&json!({"a": 1}), &json!({"a": 1, "b": 1})));
    }

    #[test]
    fn a_wrong_status_is_reported_with_the_body() {
        let error = check_response(&response(201, None), &received(400, &[], "nope")).unwrap_err();
        assert!(
            error.contains("201") && error.contains("400") && error.contains("nope"),
            "{error}"
        );
    }

    #[test]
    fn headers_match_by_name_without_case_and_fold_repeats() {
        let got = received(200, &[("X-A", "1"), ("x-a", "2"), ("x-b", " v ")], "");
        let mut expected = response(200, None);
        expected.headers = &[("x-A", "1, 2"), ("X-B", "v")];
        expected.require_headers = &["X-b"];
        assert_eq!(check_response(&expected, &got), Ok(()));
        expected.headers = &[("x-a", "1")];
        assert!(
            check_response(&expected, &got)
                .unwrap_err()
                .contains("header x-a")
        );
        expected.headers = &[];
        expected.forbid_headers = &["X-B"];
        assert!(
            check_response(&expected, &got)
                .unwrap_err()
                .contains("must not")
        );
        expected.forbid_headers = &[];
        expected.require_headers = &["x-missing"];
        assert!(
            check_response(&expected, &got)
                .unwrap_err()
                .contains("required")
        );
    }

    #[test]
    fn json_bodies_compare_as_documents_and_other_bodies_exactly() {
        let got = received(200, &[], r#"{"b":1,  "a":[true]}"#);
        assert_eq!(
            check_response(&response(200, Some(r#"{"a":[true],"b":1.0}"#)), &got),
            Ok(())
        );
        assert!(check_response(&response(200, Some(r#"{"a":[false],"b":1}"#)), &got).is_err());
        let mut text = response(200, Some("a b"));
        text.body_media_type = Some("text/plain");
        assert_eq!(check_response(&text, &received(200, &[], "a b")), Ok(()));
        assert!(check_response(&text, &received(200, &[], "a  b")).is_err());
        // No expected body makes no claim about the answered one.
        assert_eq!(check_response(&response(200, None), &got), Ok(()));
    }

    #[test]
    fn a_refusal_names_what_differs() {
        let got = received(
            400,
            &[
                ("content-type", "application/json; charset=utf-8"),
                ("x-err", "Bad"),
            ],
            r#"{"__type":"Bad","Message":"cityId is wrong"}"#,
        );
        let mut refusal = Refusal {
            code: 400,
            headers: &[("x-err", "Bad")],
            body: Some((
                "application/json",
                BodyAssertion::MessageRegex("(?i)^cityid"),
            )),
        };
        assert_eq!(check_refusal(&refusal, &got), Ok(()));
        refusal.body = Some(("application/json", BodyAssertion::MessageRegex("^nope")));
        assert!(
            check_refusal(&refusal, &got)
                .unwrap_err()
                .contains("does not match")
        );
        refusal.body = Some((
            "application/json",
            BodyAssertion::Contents(r#"{"Message":"cityId is wrong","__type":"Bad"}"#),
        ));
        assert_eq!(check_refusal(&refusal, &got), Ok(()));
        refusal.body = Some(("application/cbor", BodyAssertion::Contents("")));
        assert!(
            check_refusal(&refusal, &got)
                .unwrap_err()
                .contains("media type")
        );
        refusal.body = None;
        refusal.code = 404;
        assert!(check_refusal(&refusal, &got).unwrap_err().contains("404"));
    }

    #[test]
    fn a_message_regex_reads_the_whole_body_when_it_is_not_json() {
        let got = received(400, &[("content-type", "text/plain")], "bad label");
        let refusal = Refusal {
            code: 400,
            headers: &[],
            body: Some(("text/plain", BodyAssertion::MessageRegex("label$"))),
        };
        assert_eq!(check_refusal(&refusal, &got), Ok(()));
        let json = received(
            400,
            &[("content-type", "application/json")],
            r#"{"other":"x"}"#,
        );
        let refusal = Refusal {
            body: Some(("application/json", BodyAssertion::MessageRegex("x"))),
            ..refusal
        };
        assert!(
            check_refusal(&refusal, &json)
                .unwrap_err()
                .contains("no error message")
        );
    }
}
