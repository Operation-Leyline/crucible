//! The shared `protocol-json` fixture generated once per AWS JSON protocol
//! (`restJson1`, `awsJson1_0`, `awsJson1_1`), with its error renamed, each
//! client driven over loopback HTTP: targets and content types, timestamp
//! defaults and bindings, null handling, non-finite floats, union `__type`
//! tolerance, and error decoding.
mod common;

use std::sync::{Arc, Mutex};

use crucible_client_runtime_rust::{Config, Error, Field, Retry};
use serde_json::{Value, json};

#[allow(dead_code)]
mod rest_json {
    include!(concat!(env!("OUT_DIR"), "/protocol_restJson1_client.rs"));
}
#[allow(dead_code)]
mod aws_json_10 {
    include!(concat!(env!("OUT_DIR"), "/protocol_awsJson1_0_client.rs"));
}
#[allow(dead_code)]
mod aws_json_11 {
    include!(concat!(env!("OUT_DIR"), "/protocol_awsJson1_1_client.rs"));
}

const HTTP_DATE: &str = "Wed, 31 Dec 1969 23:59:59 GMT";

/// The server half: echoes `/echo` with a union `__type` and a null member,
/// and answers errors as the mode asks.
async fn server(mode: Arc<Mutex<&'static str>>, seen: Arc<Mutex<Vec<common::Request>>>) -> String {
    common::serve(move |request| {
        seen.lock().unwrap().push(request.clone());
        match *mode.lock().unwrap() {
            "error" => (
                409,
                vec![
                    (
                        "x-amzn-errortype".into(),
                        "example.json#Problem:detail".into(),
                    ),
                    ("x-amzn-requestid".into(), "request-42".into()),
                ],
                br#"{"__type":"Other","Message":"bad"}"#.to_vec(),
            ),
            "body-error" => (
                400,
                vec![],
                br#"{"__type":"example.json#Problem:http://internal","message":"from body"}"#
                    .to_vec(),
            ),
            "unknown" => (
                400,
                vec![("x-amzn-errortype".into(), "Throttled".into())],
                br#"{"Message":"slow down"}"#.to_vec(),
            ),
            "empty" => (200, vec![], Vec::new()),
            _ => {
                let mut body: Value = serde_json::from_slice(&request.body).unwrap();
                body["choice"] = json!({"text":"ok","__type":"ignored"});
                body["nullable"] = Value::Null;
                let headers = vec![
                    ("x-date".into(), HTTP_DATE.into()),
                    ("x-dates".into(), format!("{HTTP_DATE}, {HTTP_DATE}")),
                ];
                (200, headers, serde_json::to_vec(&body).unwrap())
            }
        }
    })
    .await
}

macro_rules! protocol_test {
    ($name:ident, $module:ident, $rpc:expr) => {
        #[tokio::test]
        async fn $name() {
            use $module::model::{Choice, ValuesInput};
            let rpc: Option<&str> = $rpc;
            let mode = Arc::new(Mutex::new("echo"));
            let seen = Arc::new(Mutex::new(Vec::new()));
            let endpoint = server(mode.clone(), seen.clone()).await;
            let client =
                $module::JsonServiceClient::new(Config::new(endpoint).retry(Retry::none()));
            let instant = chrono::DateTime::from_timestamp(-1, 750_000_000).unwrap();
            let input = ValuesInput {
                nullable: Field::Null,
                document: Field::Null,
                ..ValuesInput::builder()
                    .renamed("hello")
                    .when(instant)
                    .iso(instant)
                    .date(instant)
                    .dates(vec![instant, instant])
                    .query(instant)
                    .queries(vec![instant])
                    .float_value(f32::NAN)
                    .double_value(f64::INFINITY)
                    .build()
            };
            let output = client.echo(input).await.unwrap();
            let request = seen.lock().unwrap().last().cloned().unwrap();
            assert_eq!(request.method, "POST");
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["when"].to_string(), "-0.25");
            assert_eq!(body["iso"], "1969-12-31T23:59:59.750Z");
            assert!(body.get("nullable").is_none(), "structure null was sent");
            assert!(body.get("document").is_some_and(Value::is_null));
            assert_eq!(body["floatValue"], "NaN");
            assert_eq!(body["doubleValue"], "Infinity");
            match rpc {
                Some(content_type) => {
                    assert_eq!(request.path(), "/");
                    assert_eq!(request.header("x-amz-target"), Some("JsonService.Echo"));
                    assert_eq!(request.header("content-type"), Some(content_type));
                    assert_eq!(body["renamed"], "hello");
                    assert_eq!(body["date"].to_string(), "-0.25");
                    assert_eq!(body["queries"].to_string(), "[-0.25]");
                }
                None => {
                    assert_eq!(request.path(), "/echo");
                    assert_eq!(request.header("x-amz-target"), None);
                    assert_eq!(request.header("content-type"), Some("application/json"));
                    assert_eq!(request.header("x-date"), Some(HTTP_DATE));
                    assert_eq!(
                        request.header("x-dates"),
                        Some(format!("{HTTP_DATE}, {HTTP_DATE}").as_str())
                    );
                    assert_eq!(
                        request.query(),
                        "time=1969-12-31T23%3A59%3A59.750Z&times=1969-12-31T23%3A59%3A59.750Z"
                    );
                    assert_eq!(body["wire-name"], "hello");
                    assert!(body.get("date").is_none());
                    // HTTP dates have second precision.
                    let second = chrono::DateTime::from_timestamp(-1, 0).unwrap();
                    assert_eq!(output.date(), Some(&second));
                    assert_eq!(output.dates(), Some(&vec![second, second]));
                }
            }
            assert!(
                output.nullable.is_absent(),
                "a null structure member is absent"
            );
            assert!(output.document.is_null(), "a document keeps its null");
            assert!(output.float_value().unwrap().is_nan());
            assert_eq!(output.double_value(), Some(&f64::INFINITY));
            assert_eq!(output.choice(), Some(&Choice::Text("ok".into())));
            assert_eq!(output.when, instant);
            assert_eq!(output.iso, instant);

            // Defaults read from their own wire formats when a member is absent.
            *mode.lock().unwrap() = "empty";
            client.empty().await.unwrap();
            let request = seen.lock().unwrap().last().cloned().unwrap();
            match rpc {
                Some(_) => {
                    assert_eq!(request.body, b"{}");
                    assert_eq!(request.header("x-amz-target"), Some("JsonService.Empty"));
                }
                None => {
                    assert_eq!(request.path(), "/empty");
                    assert!(request.body.is_empty());
                }
            }

            *mode.lock().unwrap() = "error";
            let error = client
                .echo(ValuesInput::builder().build())
                .await
                .unwrap_err();
            assert_eq!(
                error.metadata().unwrap().request_id.as_deref(),
                Some("request-42")
            );
            let Error::Service(error) = error else {
                panic!("modeled AWS failure: {error:?}")
            };
            let $module::EchoError::ProblemFault(problem) = error.error;
            assert_eq!(problem.message(), Some(&"bad".to_owned()));

            *mode.lock().unwrap() = "body-error";
            let Error::Service(error) = client
                .echo(ValuesInput::builder().build())
                .await
                .unwrap_err()
            else {
                panic!("body-named failure")
            };
            let $module::EchoError::ProblemFault(problem) = error.error;
            assert_eq!(problem.message(), Some(&"from body".to_owned()));

            *mode.lock().unwrap() = "unknown";
            let Error::Unknown(error) = client
                .echo(ValuesInput::builder().build())
                .await
                .unwrap_err()
            else {
                panic!("unknown failure")
            };
            assert_eq!(error.type_name.as_deref(), Some("Throttled"));
            assert_eq!(error.message.as_deref(), Some("slow down"));
        }
    };
}

protocol_test!(rest_json_1, rest_json, None);
protocol_test!(
    aws_json_1_0,
    aws_json_10,
    Some("application/x-amz-json-1.0")
);
protocol_test!(
    aws_json_1_1,
    aws_json_11,
    Some("application/x-amz-json-1.1")
);
