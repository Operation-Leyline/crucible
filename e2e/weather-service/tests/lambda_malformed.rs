//! What a client can put in a request that `lambda_http` cannot turn into
//! one: characters a URI does not carry raw (`"`, `<`, `>`, a space) in a
//! query string, which curl and other non-browser clients send unencoded and
//! a Function URL passes on as received, and a body that claims base64 but
//! is not. Each panicked while the event was converted, before the service
//! saw it: the invocation failed and the caller got a bare 502.

use std::sync::Arc;

use crucible_server_runtime_rust::host;
use serde_json::{Value, json};
use weather_service::MemoryCities;

fn event(method: &str, query: &str, body: Option<(&str, bool)>) -> String {
    let mut event = json!({
        "version": "2.0",
        "routeKey": "$default",
        "rawPath": "/cities",
        "rawQueryString": query,
        "headers": { "content-type": "application/json" },
        "requestContext": {
            "http": { "method": method, "path": "/cities", "protocol": "HTTP/1.1", "sourceIp": "192.0.2.1" },
            "requestId": "malformed-request",
            "stage": "$default"
        },
        "isBase64Encoded": false
    });
    if let Some((body, base64)) = body {
        event["body"] = json!(body);
        event["isBase64Encoded"] = json!(base64);
    }
    event.to_string()
}

async fn answer(event: String) -> Value {
    let service = weather_service::service(Arc::new(MemoryCities::default()));
    // On its own task, so a panic is a failed assertion rather than a crash.
    let answer = tokio::spawn(async move {
        host::lambda_event_json(&service, &event)
            .await
            .map(|answer| answer.get().to_owned())
            .map_err(|error| error.to_string())
    })
    .await
    .expect("converting the event panicked")
    .expect("the event was answered");
    serde_json::from_str(&answer).unwrap()
}

#[tokio::test]
async fn query_characters_a_uri_does_not_carry_are_read_as_sent() {
    // An unknown parameter, whatever it holds, is ignored.
    for query in ["q=\"x\"", "q=a b", "q=<b>", "q=a\\b^`{|}"] {
        let answer = answer(event("GET", query, None)).await;
        assert_eq!(answer["statusCode"], 200, "{query}: {answer}");
    }
    // A known one arrives with the characters the client sent: `"5"` is not
    // an integer, and the rejection says so rather than the platform's 502.
    // `%` passes through unencoded: a malformed escape (a trailing one
    // included) is the protocol's own 400, not a conversion failure.
    for query in ["maxResults=100%", "maxResults=%zz"] {
        let answer = answer(event("GET", query, None)).await;
        assert_eq!(answer["statusCode"], 400, "{query}: {answer}");
    }
    let answer = answer(event("GET", "maxResults=\"5\"", None)).await;
    assert_eq!(answer["statusCode"], 400, "{answer}");
    let body: Value = serde_json::from_str(answer["body"].as_str().unwrap()).unwrap();
    assert_eq!(body["reason"], "cannotParse", "{body}");
}

#[tokio::test]
async fn an_event_lambda_http_cannot_convert_is_a_400_with_its_request_id() {
    let answer = answer(event("POST", "", Some(("!!! not base64", true)))).await;
    assert_eq!(answer["statusCode"], 400, "{answer}");
    assert_eq!(
        answer["headers"]["x-request-id"], "malformed-request",
        "{answer}"
    );
}

/// The message of the host's own 400 for an event it cannot read.
const UNREADABLE: &str = "the request could not be read";

thread_local! {
    /// Panics on this test's thread: the spawned task runs on it too, since
    /// `#[tokio::test]` is single-threaded.
    static PANICS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Answers `event` and asserts nothing panicked on the way: under
/// `panic = "abort"` a panic ends the process, so the host refuses what
/// lambda_http panics on before converting it rather than catching it.
async fn answer_without_panicking(event: Value) -> Value {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANICS.with(|panics| panics.set(panics.get() + 1));
            default(info);
        }));
    });
    let before = PANICS.with(std::cell::Cell::get);
    let answer = answer(event.to_string()).await;
    assert_eq!(
        PANICS.with(std::cell::Cell::get),
        before,
        "converting the event panicked: {event}"
    );
    answer
}

fn rest_event(headers: Value, body: Option<(&str, bool)>) -> Value {
    let mut event = json!({
        "httpMethod": "POST",
        "path": "/cities",
        "headers": headers,
        "requestContext": { "stage": "$default", "requestId": "rest-request", "httpMethod": "POST" },
        "isBase64Encoded": false
    });
    if let Some((body, base64)) = body {
        event["body"] = json!(body);
        event["isBase64Encoded"] = json!(base64);
    }
    event
}

fn alb_event(headers: Value, body: Option<(&str, bool)>) -> Value {
    let mut event = json!({
        "requestContext": { "elb": { "targetGroupArn": "arn:aws:elasticloadbalancing:us-east-1:123456789012:targetgroup/weather/0123456789abcdef" } },
        "httpMethod": "POST",
        "path": "/cities",
        "queryStringParameters": {},
        "headers": headers,
        "isBase64Encoded": false
    });
    if let Some((body, base64)) = body {
        event["body"] = json!(body);
        event["isBase64Encoded"] = json!(base64);
    }
    event
}

#[tokio::test]
async fn a_body_that_is_not_base64_is_refused_before_the_conversion_from_any_source() {
    let not_base64 = Some(("!!! not base64", true));
    let events = [
        serde_json::from_str(&event("POST", "", not_base64)).unwrap(),
        rest_event(json!({}), not_base64),
        alb_event(json!({}), not_base64),
    ];
    for event in events {
        let answer = answer_without_panicking(event.clone()).await;
        assert_eq!(answer["statusCode"], 400, "{event}: {answer}");
        let body: Value = serde_json::from_str(answer["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["message"], UNREADABLE, "{event}: {answer}");
    }
    // The REST API's request id still names it.
    let answer = answer_without_panicking(rest_event(json!({}), not_base64)).await;
    assert_eq!(
        answer["multiValueHeaders"]["x-request-id"][0], "rest-request",
        "{answer}"
    );
}

#[tokio::test]
async fn a_body_that_is_base64_is_decoded_from_any_source() {
    // `{}` encoded: a CreateCity input missing its required members, so the
    // service read the decoded JSON and refused its content.
    let encoded = Some(("e30=", true));
    for event in [
        serde_json::from_str(&event("POST", "", encoded)).unwrap(),
        rest_event(json!({ "content-type": "application/json" }), encoded),
        alb_event(json!({ "content-type": "application/json" }), encoded),
    ] {
        let answer = answer_without_panicking(event.clone()).await;
        assert_eq!(answer["statusCode"], 400, "{event}: {answer}");
        let body: Value = serde_json::from_str(answer["body"].as_str().unwrap()).unwrap();
        assert_ne!(body["reason"], "cannotParse", "{event}: {body}");
        assert_ne!(body["message"], UNREADABLE, "{event}: {body}");
    }
}

#[tokio::test]
async fn a_host_the_uri_cannot_be_built_from_is_refused_before_the_conversion() {
    for headers in [
        json!({ "host": "exa mple.com" }),
        json!({ "host": "example.com:port" }),
        json!({ "host": "example.com", "x-forwarded-proto": "ht tp" }),
    ] {
        for event in [
            rest_event(headers.clone(), None),
            alb_event(headers.clone(), None),
        ] {
            let answer = answer_without_panicking(event.clone()).await;
            assert_eq!(answer["statusCode"], 400, "{event}: {answer}");
            let body: Value = serde_json::from_str(answer["body"].as_str().unwrap()).unwrap();
            assert_eq!(body["message"], UNREADABLE, "{event}: {answer}");
        }
    }
    // An ordinary Host is served as it was.
    let headers = json!({ "host": "example.com", "x-forwarded-proto": "https" });
    let answer = answer_without_panicking(alb_event(headers, Some(("{}", false)))).await;
    let body: Value = serde_json::from_str(answer["body"].as_str().unwrap()).unwrap();
    assert_ne!(body["message"], UNREADABLE, "{answer}");
}
