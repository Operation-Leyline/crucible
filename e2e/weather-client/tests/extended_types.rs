use crucible_client_runtime_rust::{BigDecimal, BigInteger, Config, Field};
use crucible_server_runtime_rust::validate::Validator;
use serde_json::{Value, json};
use std::sync::Arc;

#[allow(dead_code)]
mod client {
    include!(concat!(env!("OUT_DIR"), "/types_client.rs"));
}
#[allow(dead_code)]
mod generated_server {
    include!(concat!(env!("OUT_DIR"), "/types_server.rs"));
}
use generated_server::types as server;

const INTEGER: &str = "12345678901234567890123456789012345678901234567890";
const DECIMAL: &str = "12345678901234567890.12345678901234567890123456789";

#[test]
fn exact_defaults_and_open_integer_enum() {
    let value: client::model::Output = serde_json::from_str("{}").unwrap();
    assert_eq!(value.integer.to_string(), INTEGER);
    assert_eq!(value.decimal.to_string(), DECIMAL);
    let wire = serde_json::to_string(&value).unwrap();
    assert!(wire.contains(&format!("\"integer\":{INTEGER}")));
    assert!(wire.contains(&format!("\"decimal\":{DECIMAL}")));
    let value: client::model::Output = serde_json::from_str(
        r#"{"mode":77,"sparseMap":{"null":null,"future":77},"sparseNames":[null,"x"]}"#,
    )
    .unwrap();
    assert_eq!(value.mode.value().unwrap().as_i32(), 77);
    assert_eq!(
        value.sparse_map.value().unwrap()["future"]
            .unwrap()
            .as_i32(),
        77
    );
    assert!(serde_json::from_str::<server::Output>(r#"{"mode":77}"#).is_err());
    assert!(serde_json::from_str::<client::model::Output>(r#"{"mode":2147483648}"#).is_err());
    let integer: BigInteger = serde_json::from_str("12345678901234567890e20").unwrap();
    assert_eq!(
        integer.to_string(),
        "1234567890123456789000000000000000000000"
    );
    assert!(serde_json::from_str::<BigInteger>("1.5").is_err());
}

#[test]
fn collections_enforce_value_uniqueness_and_preserve_nested_document_null() {
    for text in [
        r#"{"names":["x","x"]}"#,
        r#"{"numbers":[1,1.00]}"#,
        r#"{"documents":[{"x":1,"y":2},{"y":2,"x":1.0}]}"#,
        r#"{"names":[null]}"#,
        r#"{"documents":[null]}"#,
        r#"{"choice":{"numbers":[1,1e0]}}"#,
    ] {
        assert!(
            serde_json::from_str::<client::model::Output>(text).is_err(),
            "{text}"
        );
        assert!(
            serde_json::from_str::<server::Output>(text).is_err(),
            "{text}"
        );
    }
    let value: client::model::Output =
        serde_json::from_str(r#"{"documents":[{"nil":null},{}],"names":["a","b"]}"#).unwrap();
    assert_eq!(value.documents.value().unwrap()[0]["nil"], Value::Null);
    let mut duplicate = value;
    duplicate.numbers = Field::Value(vec!["1".parse().unwrap(), "1.00".parse().unwrap()]);
    assert!(serde_json::to_string(&duplicate).is_err());
}

#[test]
fn nested_collection_timestamp_codecs_and_exact_ranges() {
    let text =
        r#"{"dates":{"x":[null,1700000000.123456789]},"choice":{"dates":{"y":[-0.000000001]}}}"#;
    let value: client::model::Output = serde_json::from_str(text).unwrap();
    assert_eq!(
        value.dates.value().unwrap()["x"][1]
            .unwrap()
            .timestamp_subsec_nanos(),
        123456789
    );
    let encoded = serde_json::to_string(&value).unwrap();
    assert!(encoded.contains("1700000000.123456789"));
    assert!(encoded.contains("-0.000000001"));
    let server: server::Output = serde_json::from_str(text).unwrap();
    assert_eq!(
        serde_json::to_value(&server).unwrap()["choice"],
        serde_json::from_str::<Value>(text).unwrap()["choice"]
    );
    for (decimal, valid) in [
        ("12345678901234567890.000000001", true),
        ("12345678901234567889.999999999", false),
        ("12345678901234567891.000000001", false),
    ] {
        let input: server::Input =
            serde_json::from_str(&format!("{{\"decimal\":{decimal}}}")).unwrap();
        assert_eq!(Validator::check(&input).is_ok(), valid, "{decimal}");
    }
}

struct Types;
impl server::ReadOperation for Types {
    async fn read(&self) -> crucible_server_runtime_rust::Result<server::Output> {
        Ok(serde_json::from_str("{}").unwrap())
    }
}
impl server::UpdateOperation for Types {
    async fn update(
        &self,
        input: server::Input,
    ) -> crucible_server_runtime_rust::Result<server::Output> {
        assert_eq!(input.decimal.to_string(), DECIMAL);
        Ok(serde_json::from_value(json!({"sparseNames":[null,"native"],"mode":-1})).unwrap())
    }
}

#[tokio::test]
async fn generated_extended_types_round_trip_over_http() {
    let service =
        crucible_server_runtime_rust::Crucible::builder(server::operations(Arc::new(Types)))
            .protocol(crucible_server_runtime_rust::JsonProtocol)
            .on_rejection(|_| Box::new(serde_json::from_str::<server::Bad>("{}").unwrap()))
            .build()
            .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(crucible_server_runtime_rust::host::http_on(
        service, listener,
    ));
    let client = client::TypesClient::new(Config::new(endpoint));
    let output = client.read().await.unwrap();
    assert_eq!(output.integer.to_string(), INTEGER);
    assert_eq!(output.decimal, DECIMAL.parse::<BigDecimal>().unwrap());
    let output = client
        .update(
            client::model::Input::builder()
                .decimal(DECIMAL.parse::<BigDecimal>().unwrap())
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(
        output.sparse_names.value().unwrap(),
        &[None, Some("native".into())]
    );
}
