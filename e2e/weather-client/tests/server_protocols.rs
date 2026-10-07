//! Generated Rust clients against the native phased server, over real HTTP.
use crucible_client_runtime_rust::{Config, Error, Retry};
mod common;

macro_rules! service {
    ($module:ident, $client:literal, $server:literal, $error:ident, $($extra:ident),*) => {
        #[allow(dead_code)]
        mod $module {
            pub mod client { include!(concat!(env!("OUT_DIR"), $client)); }
            mod generated_server { include!(concat!(env!("OUT_DIR"), $server)); }
            pub use generated_server::json_service as server;

            struct Service;
            impl server::EchoOperation for Service {
                async fn echo(&self, input: server::ValuesInput) -> crucible_server_runtime_rust::Result<server::ValuesOutput> {
                    match input.renamed.as_deref() {
                        Some("error") => return Err(server::$error { message: Some("modeled refusal".to_owned()) }.into()),
                        Some("internal") => return Err(std::io::Error::other("private server detail").into()),
                        _ => {}
                    }
                    Ok(server::ValuesOutput {
                        renamed: input.renamed, when: input.when, iso: input.iso,
                        date: input.date, dates: input.dates, choice: input.choice,
                        float_value: input.float_value, double_value: input.double_value,
                        nullable: input.nullable, $($extra: input.$extra,)*
                    })
                }
            }
            impl server::EmptyOperation for Service {
                async fn empty(&self) -> crucible_server_runtime_rust::Result<()> { Ok(()) }
            }
            pub async fn serve() -> String {
                let service = crucible_server_runtime_rust::Crucible::builder(server::operations(std::sync::Arc::new(Service)))
                    .defaults()
                    .on_rejection(|rejection| Box::new(server::$error { message: Some(rejection.summary()) }))
                    .build().unwrap();
                super::serve(service).await
            }
        }
    };
}

service!(
    rest,
    "/protocol_restJson1_client.rs",
    "/protocol_restJson1_server.rs",
    ProblemFault,
    document
);
service!(
    aws10,
    "/protocol_awsJson1_0_client.rs",
    "/protocol_awsJson1_0_server.rs",
    ProblemFault,
    document
);
service!(
    aws11,
    "/protocol_awsJson1_1_client.rs",
    "/protocol_awsJson1_1_server.rs",
    ProblemFault,
    document
);
service!(
    rpc_json,
    "/protocol_rpcv2Json_client.rs",
    "/protocol_rpcv2Json_server.rs",
    Problem,
    document,
    blob,
    large,
    decimal
);
service!(
    rpc_cbor,
    "/protocol_rpcv2Cbor_client.rs",
    "/protocol_rpcv2Cbor_server.rs",
    Problem,
    blob,
    large,
    decimal
);

async fn serve(service: crucible_server_runtime_rust::Crucible) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(crucible_server_runtime_rust::host::http_on(
        service, listener,
    ));
    endpoint
}

macro_rules! round_trip {
    ($test:ident, $module:ident, $rest:expr, $error:ident, $status:expr) => {
        #[tokio::test]
        async fn $test() {
            use $module::client::model::{Choice, ValuesInput};
            let endpoint = $module::serve().await;
            let client =
                $module::client::JsonServiceClient::new(Config::new(endpoint).retry(Retry::none()));
            let when = chrono::DateTime::from_timestamp(-1, 750_000_000).unwrap();
            let input = ValuesInput::builder()
                .renamed("hello")
                .when(when)
                .iso(when)
                .date(when)
                .dates(vec![when])
                .choice(Choice::Text("arm".to_owned()))
                .float_value(f32::NAN)
                .double_value(f64::INFINITY)
                .build();
            let output = client.echo(input).await.unwrap();
            assert_eq!(output.renamed(), Some(&"hello".to_owned()));
            assert_eq!(output.when, when);
            assert_eq!(output.iso, when);
            let bound_date = if $rest {
                chrono::DateTime::from_timestamp(-1, 0).unwrap()
            } else {
                when
            };
            assert_eq!(output.date(), Some(&bound_date));
            assert_eq!(output.dates(), Some(&vec![bound_date]));
            assert_eq!(output.choice(), Some(&Choice::Text("arm".to_owned())));
            assert!(output.float_value().unwrap().is_nan());
            assert_eq!(output.double_value(), Some(&f64::INFINITY));
            client.empty().await.unwrap();

            let error = client
                .echo(ValuesInput::builder().renamed("error").build())
                .await
                .unwrap_err();
            assert_eq!(error.metadata().unwrap().status, $status);
            assert!(error.metadata().unwrap().request_id.is_some());
            let Error::Service(error) = error else {
                panic!("modeled error was lost: {error:?}");
            };
            let $module::client::EchoError::$error(error) = error.error;
            assert_eq!(error.message(), Some(&"modeled refusal".to_owned()));

            let error = client
                .echo(ValuesInput::builder().renamed("internal").build())
                .await
                .unwrap_err();
            let Error::Unknown(error) = error else {
                panic!("unmodeled error: {error:?}");
            };
            assert_eq!(error.metadata.status, 500);
            assert!(!error.message.as_deref().unwrap().contains("private"));
        }
    };
}

round_trip!(rest_json_round_trip, rest, true, ProblemFault, 409);
round_trip!(aws_json_10_round_trip, aws10, false, ProblemFault, 400);
round_trip!(aws_json_11_round_trip, aws11, false, ProblemFault, 400);
round_trip!(rpc_v2_json_round_trip, rpc_json, false, Problem, 409);
round_trip!(rpc_v2_cbor_round_trip, rpc_cbor, false, Problem, 409);

macro_rules! precision {
    ($test:ident, $module:ident) => {
        #[tokio::test]
        async fn $test() {
            let client = $module::client::JsonServiceClient::new(
                Config::new($module::serve().await).retry(Retry::none()),
            );
            let integer: crucible_client_runtime_rust::BigInteger =
                "-12345678901234567890123456789012345678901234567890"
                    .parse()
                    .unwrap();
            let decimal: crucible_client_runtime_rust::BigDecimal =
                "12345678901234567890.1234567890123456789".parse().unwrap();
            let blob = crucible_client_runtime_rust::Blob::new(vec![0, 255, 128, 42]);
            let output = client
                .echo(
                    $module::client::model::ValuesInput::builder()
                        .blob(blob.clone())
                        .large(integer.clone())
                        .decimal(decimal.clone())
                        .build(),
                )
                .await
                .unwrap();
            assert_eq!(output.blob(), Some(&blob));
            assert_eq!(output.large(), Some(&integer));
            assert_eq!(output.decimal(), Some(&decimal));
        }
    };
}
precision!(rpc_v2_json_preserves_precision, rpc_json);
precision!(rpc_v2_cbor_preserves_native_values, rpc_cbor);

#[tokio::test]
async fn rpc_client_checks_response_protocol_before_decoding() {
    for protocol in [None, Some("rpc-v2-cbor")] {
        let endpoint = common::serve(move |_| {
            let headers = protocol
                .map(|value| vec![("smithy-protocol".into(), value.into())])
                .unwrap_or_default();
            (
                409,
                headers,
                br#"{"__type":"example.json#Problem","message":"do not decode"}"#.to_vec(),
            )
        })
        .await;
        let client =
            rpc_json::client::JsonServiceClient::new(Config::new(endpoint).retry(Retry::none()));
        let error = client
            .echo(rpc_json::client::model::ValuesInput::builder().build())
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Decode(message) if message.contains("HTTP 409") && message.contains("smithy-protocol"))
        );
    }
}

#[tokio::test]
async fn rpc_errors_require_the_full_matching_shape_id() {
    let endpoint = common::serve(|_| {
        (
            409,
            vec![
                ("smithy-protocol".into(), "rpc-v2-json".into()),
                ("content-type".into(), "application/json".into()),
                ("x-amzn-errortype".into(), "Problem".into()),
            ],
            br#"{"__type":"unrelated.namespace#Problem","message":"foreign error"}"#.to_vec(),
        )
    })
    .await;
    let client =
        rpc_json::client::JsonServiceClient::new(Config::new(endpoint).retry(Retry::none()));
    let error = client
        .echo(rpc_json::client::model::ValuesInput::builder().build())
        .await
        .unwrap_err();
    assert!(
        matches!(error, Error::Unknown(error) if error.type_name.as_deref() == Some("unrelated.namespace#Problem"))
    );
}

#[tokio::test]
async fn aws_rpc_claims_bad_targets_without_falling_through() {
    let endpoint = aws10::serve().await;
    // Raw reqwest clients need the runtime's TLS provider even over HTTP.
    drop(Config::new(&endpoint));
    let http = reqwest::Client::new();
    for target in [
        None,
        Some("JsonService"),
        Some("JsonService.Echo.extra"),
        Some("Other.Echo"),
        Some("JsonService.Missing"),
    ] {
        let mut request = http
            .post(&endpoint)
            .header("content-type", "application/x-amz-json-1.0");
        if let Some(target) = target {
            request = request.header("x-amz-target", target);
        }
        let response = request.body("{}").send().await.unwrap();
        assert!(response.status().is_client_error());
        assert_eq!(
            response.headers()["content-type"],
            "application/x-amz-json-1.0"
        );
    }
    let response = http
        .post(&endpoint)
        .header("content-type", "application/x-amz-json-1.0; charset=utf-8")
        .header("x-amz-target", "JsonService.Echo")
        .body("{broken")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    assert_eq!(response.headers()["x-amzn-errortype"], "Problem");
    let body: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(body["__type"], "example.json#Problem");
}

#[tokio::test]
async fn rpc_v2_rejects_cross_protocol_headers_and_preserves_prefix_routing() {
    let endpoint = rpc_json::serve().await;
    drop(Config::new(&endpoint));
    let http = reqwest::Client::new();
    for target in ["x-amz-target", "x-amzn-target"] {
        let response = http
            .post(format!("{endpoint}/service/JsonService/operation/Echo"))
            .header("smithy-protocol", "rpc-v2-json")
            .header("content-type", "application/json")
            .header(target, "JsonService.Echo")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 409);
        assert_eq!(response.headers()["smithy-protocol"], "rpc-v2-json");
        assert!(!response.headers().contains_key("x-amzn-errortype"));
    }
    for service in ["JsonService", "example.json.JsonService"] {
        let response = http
            .post(format!("{endpoint}/v1/service/{service}/operation/Echo"))
            .header("smithy-protocol", "rpc-v2-json")
            .header("content-type", "application/json")
            .body(r#"{"renamed":"ok","choice":{"__type":"ignored","text":"arm"}}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(body["renamed"], "ok");
    }
    let response = http
        .post(format!("{endpoint}/service/JsonService/operation/Empty"))
        .header("smithy-protocol", "rpc-v2-json")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(!response.headers().contains_key("content-type"));
    assert!(response.bytes().await.unwrap().is_empty());
}

#[tokio::test]
async fn cbor_trailing_or_truncated_documents_are_rejected() {
    let endpoint = rpc_cbor::serve().await;
    drop(Config::new(&endpoint));
    for body in [vec![0xa0, 0xa0], vec![0xa1], vec![0xff]] {
        let response = reqwest::Client::new()
            .post(format!("{endpoint}/service/JsonService/operation/Echo"))
            .header("smithy-protocol", "rpc-v2-cbor")
            .header("content-type", "application/cbor")
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 409);
        assert_eq!(response.headers()["content-type"], "application/cbor");
        let body = response.bytes().await.unwrap();
        assert_eq!(
            crucible_server_runtime_rust::cbor::error_envelope(&body).unwrap()["__type"],
            "example.json#Problem"
        );
    }
}
