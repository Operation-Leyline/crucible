use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::middlewares::{LogPolicy, LogRecord, LogSink, ServiceLog};
use crucible_server_runtime_rust::{Crucible, InvokeProtocol, JsonProtocol, Operation};
use serde_json::{Value, json};

mod generated_api {
    include!(concat!(env!("OUT_DIR"), "/observability.rs"));
}
use generated_api::observability as api;
mod client {
    include!(concat!(env!("OUT_DIR"), "/observability_client.rs"));
}

struct Echo;
impl api::EchoOperation for Echo {
    async fn echo(
        &self,
        input: api::Payload,
    ) -> crucible_server_runtime_rust::Result<api::Payload> {
        if input.profile.as_ref().and_then(|p| p.name.as_deref()) == Some("fail") {
            return Err(api::SecretError {
                profile: input.profile,
                message: Some("conflict".into()),
            }
            .into());
        }
        Ok(input)
    }
}
impl api::OtherOperation for Echo {
    async fn other(
        &self,
        input: api::Payload,
    ) -> crucible_server_runtime_rust::Result<api::Payload> {
        Ok(input)
    }
}

#[derive(Clone, Default)]
struct Records(Arc<Mutex<Vec<Value>>>);
impl LogSink for Records {
    fn write(&self, record: &LogRecord<'_>) {
        self.0
            .lock()
            .unwrap()
            .push(serde_json::to_value(record).unwrap());
    }
}

fn service(log: impl crucible_server_runtime_rust::Middleware) -> Crucible {
    Crucible::builder(api::operations(Arc::new(Echo)))
        .middleware(log)
        .protocol(JsonProtocol)
        .protocol(InvokeProtocol)
        .build()
        .unwrap()
}

#[tokio::test]
async fn generated_redaction_preserves_public_fields_and_never_changes_the_response() {
    let records = Records::default();
    let service = service(ServiceLog::new(LogPolicy::everything()).sink(records.clone()));
    let profile = json!({"name":"public", "password":"secret", "child":{"name":"child", "password":"secret"}});
    let payload = json!({"profile":profile, "profiles":[profile], "byName":{"one":profile},
        "choice":{"profile":profile}, "hiddenKeys":{"secret":"value"}, "whole":{"value":"secret"}, "secretTarget":"secret"});
    let output = service.invoke("Echo", payload.clone()).await;
    assert_eq!(output["output"], payload);
    service
        .invoke(
            "Echo",
            json!({"profile":{"name":"fail", "password":"secret"}}),
        )
        .await;
    service
        .invoke("Echo", json!({"choice":{"secret":"secret"}}))
        .await;
    let records = records.0.lock().unwrap();
    assert_eq!(records.len(), 3);
    let encoded = serde_json::to_string(&*records).unwrap();
    assert!(!encoded.contains("secret"), "{encoded}");
    for field in ["input", "output"] {
        assert_eq!(records[0][field]["profile"]["name"], "public");
        assert_eq!(records[0][field]["profiles"][0]["child"]["name"], "child");
        assert_eq!(records[0][field]["byName"]["one"]["name"], "public");
        assert_eq!(records[0][field]["choice"]["profile"]["name"], "public");
    }
    assert_eq!(records[1]["errorFields"]["profile"], json!({"name":"fail"}));
}

#[tokio::test]
async fn literal_queries_select_the_operation_and_the_generated_client_sends_them() {
    let records = Records::default();
    let service = service(ServiceLog::new(LogPolicy::everything()).sink(records.clone()));
    for (uri, expected) in [
        ("/echo", 404),
        ("/echo?mode=wrong&flag", 404),
        ("/echo?mode=echo", 404),
        ("/echo?mode=echo&flag", 200),
        ("/echo?mode=other", 200),
    ] {
        let response = service
            .handle(Request::post(uri).body(Body::from("{}")).unwrap())
            .await;
        assert_eq!(response.status().as_u16(), expected, "{uri}");
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(crucible_server_runtime_rust::host::http_on(
        service, listener,
    ));
    let client =
        client::ObservabilityClient::new(crucible_client_runtime_rust::Config::new(endpoint));
    let payload = || serde_json::from_value::<client::model::Payload>(json!({})).unwrap();
    client.echo(payload()).await.unwrap();
    client.other(payload()).await.unwrap();
    server.abort();
    let records = records.0.lock().unwrap();
    assert_eq!(records[5]["operation"], "Echo");
    assert_eq!(records[6]["operation"], "Other");
}

#[test]
fn exact_query_values_outrank_any_value_matches() {
    use crucible_server_runtime_rust::protocol::Protocol;

    struct AnyMode;
    impl Operation for AnyMode {
        const INFO: &'static crucible_server_runtime_rust::OperationInfo =
            &crucible_server_runtime_rust::OperationInfo {
                literal_query: &[("mode", None)],
                ..*api::Echo::INFO
            };
        type Input = api::Payload;
        type Output = api::Payload;
    }

    // Echo sorts before Other, so a tie would incorrectly select Echo.
    let operations = crucible_server_runtime_rust::Operations::builder(&api::SERVICE)
        .add::<AnyMode, _>(Arc::new(()), |_, input| Box::pin(async { Ok(input) }))
        .add::<api::Other, _>(Arc::new(()), |_, input| Box::pin(async { Ok(input) }))
        .build()
        .unwrap();
    for (uri, expected) in [
        ("/echo?mode=other", Some("Other")),
        ("/echo?mode=oth%65r", Some("Other")),
        ("/echo?mode=echo", Some("Echo")),
        ("/echo?mode", Some("Echo")),
        ("/echo", None),
    ] {
        let (request, _) = Request::post(uri).body(()).unwrap().into_parts();
        let route = JsonProtocol.route(&request, &operations);
        assert_eq!(route.map(|route| route.operation), expected, "{uri}");
    }
}

#[tokio::test]
async fn malformed_query_data_preserves_valid_literals_and_rejection_operation_context() {
    use crucible_server_runtime_rust::protocol::Protocol;

    let operations = api::operations(Arc::new(Echo));
    for (uri, expected) in [
        ("/echo?mode=other&ignored=%FF", Some("Other")),
        ("/echo?mode=other&%GG=flag", Some("Other")),
        ("/echo?mode=echo&flag&ignored=%FF", Some("Echo")),
        ("/echo?mode=echo&ignored=%FF", None),
        ("/echo?mode=wrong&ignored=%FF", None),
        ("/echo?mode=%FF&flag", Some("Echo")),
        ("/echo?%GG=other", Some("Echo")),
        ("/echo?mode=other&mode=echo&flag&ignored=%FF", Some("Echo")),
        ("/echo?mode=wrong&mode=other&ignored=%FF", Some("Other")),
        ("/echo?mode=echo&flag=%FF", Some("Echo")),
        ("/echo?mode=other&flag=%FF", Some("Other")),
    ] {
        let (request, _) = Request::post(uri).body(()).unwrap().into_parts();
        let route = JsonProtocol.route(&request, &operations);
        assert_eq!(
            route.as_ref().map(|route| route.operation),
            expected,
            "{uri}"
        );
        if let Some(route) = route {
            assert!(route.rejection.unwrap().is_malformed());
        }
    }
    let records = Records::default();
    let service = service(ServiceLog::new(LogPolicy::everything()).sink(records.clone()));
    let response = service
        .handle(
            Request::post("/echo?mode=other&ignored=%FF")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), 400);
    let records = records.0.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["operation"], "Other");
    assert_eq!(records[0]["status"], 400);
}

struct BrokenOutput;
impl serde::Serialize for BrokenOutput {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("cannot serialize test output"))
    }
}
struct BrokenEcho;
impl Operation for BrokenEcho {
    const INFO: &'static crucible_server_runtime_rust::OperationInfo = api::Echo::INFO;
    type Input = api::Payload;
    type Output = BrokenOutput;
}

#[tokio::test]
async fn serialization_faults_are_logged_after_writing_and_scopes_still_apply() {
    let records = Records::default();
    let operations = crucible_server_runtime_rust::Operations::builder(&api::SERVICE)
        .add::<BrokenEcho, _>(Arc::new(()), |_, _| Box::pin(async { Ok(BrokenOutput) }))
        .add::<api::Other, _>(Arc::new(()), |_, input| Box::pin(async { Ok(input) }))
        .build()
        .unwrap();
    let service = Crucible::builder(operations)
        .middleware(
            ServiceLog::new(LogPolicy::off())
                .operation::<api::Echo>(LogPolicy::errors_only())
                .sink(records.clone()),
        )
        .protocol(JsonProtocol)
        .protocol(InvokeProtocol)
        .build()
        .unwrap();
    let response = service
        .handle(
            Request::post("/echo?mode=echo&flag")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), 500);
    let response = service.invoke("Echo", json!({})).await;
    assert_eq!(response["error"]["__type"], "InternalFailure");
    service.invoke("Other", json!({})).await;
    let records = records.0.lock().unwrap();
    assert_eq!(records.len(), 2);
    for record in records.iter() {
        assert_eq!(record["status"], 500);
        assert_eq!(record["outcome"], "fault");
        assert_eq!(record["cause"], "cannot serialize test output");
    }
}
