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
