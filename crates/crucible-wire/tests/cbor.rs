use ciborium::Value;
use crucible_wire::{
    blob::Blob,
    cbor,
    number::{BigDecimal, BigInteger},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Native {
    blob: Blob,
    #[serde(with = "crucible_wire::codec::Adapter::<cbor::Timestamp>")]
    timestamp: crucible_wire::time::Timestamp,
    #[serde(with = "crucible_wire::codec::Adapter::<cbor::Integer>")]
    integer: BigInteger,
    #[serde(with = "crucible_wire::codec::Adapter::<cbor::Decimal>")]
    decimal: BigDecimal,
    #[serde(with = "crucible_wire::codec::Adapter::<crucible_wire::codec::NonFinite>")]
    number: f64,
}

#[test]
fn native_tags_bytes_and_non_finite_floats_are_not_json_proxies() {
    let input = Native {
        blob: Blob::new(vec![0, 255]),
        timestamp: chrono::DateTime::from_timestamp(-1, 750_000_000).unwrap(),
        integer: "-18446744073709551617".parse().unwrap(),
        decimal: "273.15".parse().unwrap(),
        number: f64::NAN,
    };
    let mut bytes = Vec::new();
    cbor::write(&input, &mut bytes).unwrap();
    let Value::Map(fields) = cbor::read(&bytes).unwrap() else {
        panic!("map")
    };
    let get = |name: &str| {
        &fields
            .iter()
            .find(|(key, _)| key == &Value::Text(name.into()))
            .unwrap()
            .1
    };
    assert_eq!(get("blob"), &Value::Bytes(vec![0, 255]));
    assert_eq!(
        get("timestamp"),
        &Value::Tag(1, Box::new(Value::Float(-0.25)))
    );
    assert_eq!(
        get("integer"),
        &Value::Tag(3, Box::new(Value::Bytes(vec![1, 0, 0, 0, 0, 0, 0, 0, 0])))
    );
    assert!(matches!(get("decimal"), Value::Tag(4, _)));
    assert!(matches!(get("number"), Value::Float(value) if value.is_nan()));
    let output: Native = cbor::read(&bytes).unwrap();
    assert_eq!(output.blob, input.blob);
    assert_eq!(output.timestamp, input.timestamp);
    assert_eq!(output.integer, input.integer);
    assert_eq!(output.decimal, input.decimal);
    assert!(output.number.is_nan());
    let mut error = Vec::new();
    cbor::write_typed(&input, "example#NativeError", &mut error).unwrap();
    let output: Native = cbor::read(&error).unwrap();
    assert_eq!(output.blob, input.blob);
    assert_eq!(
        cbor::error_envelope(&error).unwrap()["__type"],
        "example#NativeError"
    );
}

#[test]
fn normative_decimal_and_integer_tags_decode() {
    #[derive(Deserialize)]
    struct Decimal(#[serde(with = "crucible_wire::codec::Adapter::<cbor::Decimal>")] BigDecimal);
    #[derive(Deserialize)]
    struct Integer(#[serde(with = "crucible_wire::codec::Adapter::<cbor::Integer>")] BigInteger);
    assert_eq!(
        cbor::read::<Decimal>(&[0xc4, 0x82, 0x21, 0x19, 0x6a, 0xb3])
            .unwrap()
            .0,
        "273.15".parse().unwrap()
    );
    assert_eq!(
        cbor::read::<Integer>(&[0xc2, 0x49, 1, 0, 0, 0, 0, 0, 0, 0, 0])
            .unwrap()
            .0
            .to_string(),
        "18446744073709551616"
    );
    assert!(cbor::read::<Value>(&[0xa0, 0xa0]).is_err());
    assert!(cbor::read::<Decimal>(&[0xc4, 0x81, 0x21]).is_err());
}

#[test]
fn rpc_json_big_numbers_obey_the_protocol_grammar() {
    #[derive(Deserialize)]
    struct Integer(
        #[serde(with = "crucible_wire::codec::Adapter::<cbor::StringNumber>")] BigInteger,
    );
    #[derive(Deserialize)]
    struct Decimal(
        #[serde(with = "crucible_wire::codec::Adapter::<cbor::StringNumber>")] BigDecimal,
    );
    for invalid in ["+1", "01", "1.0", "1e2", " 1", "1 "] {
        assert!(
            serde_json::from_value::<Integer>(serde_json::json!(invalid)).is_err(),
            "{invalid}"
        );
    }
    assert_eq!(
        serde_json::from_str::<Integer>("\"-123456789012345678901234567890\"")
            .unwrap()
            .0
            .to_string(),
        "-123456789012345678901234567890"
    );
    assert_eq!(
        serde_json::from_str::<Decimal>("\"-1.25e+3\"").unwrap().0,
        "-1250".parse().unwrap()
    );
}
