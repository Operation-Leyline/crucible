//! Shared client serialization: every corpus case runs against native models.
// The fixture deprecates `createdAt`; the corpus still covers it.
#![allow(deprecated)]
use crucible_client_runtime_rust::__generated::serde::{Serialize, de::DeserializeOwned};
use crucible_client_runtime_rust::Field;
use serde_json::Value;
use weather_client::{CitySummary, GetCityOutput, Location, Photo, Sky};

const PROFILE: &str = "rust-chrono-ns";

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../tests/conformance/serialization.json"
    ))
    .unwrap()
}

// Normalize decimal spelling without converting exact integers through f64.
fn number(text: &str) -> (String, i64) {
    let negative = text.starts_with('-');
    let text = text.trim_start_matches('-');
    let (coefficient, exponent) = text
        .split_once(['e', 'E'])
        .map_or((text, 0), |(coefficient, power)| {
            (coefficient, power.parse::<i64>().unwrap())
        });
    let fraction = coefficient
        .split_once('.')
        .map_or(0, |(_, tail)| tail.len());
    let digits = coefficient.replace('.', "");
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return ("0".into(), 0);
    }
    let trimmed = digits.trim_end_matches('0');
    (
        format!("{}{trimmed}", if negative { "-" } else { "" }),
        exponent - fraction as i64 + (digits.len() - trimmed.len()) as i64,
    )
}

fn equal_json(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => number(&a.to_string()) == number(&b.to_string()),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equal_json(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| equal_json(value, other)))
        }
        _ => left == right,
    }
}

fn round_trip<T: DeserializeOwned + Serialize>(row: &Value, observe: impl FnOnce(&T, &Value)) {
    let decoded = serde_json::from_str::<T>(row["wire"].as_str().unwrap());
    if row["reject"] == true {
        assert!(decoded.is_err(), "{}: invalid value accepted", row["id"]);
        return;
    }
    let native = decoded.unwrap_or_else(|error| panic!("{}: {error}", row["id"]));
    let expect = row["expectByProfile"]
        .get(PROFILE)
        .unwrap_or(&row["expect"]);
    let expected: Value = serde_json::from_str(expect["json"].as_str().unwrap()).unwrap();
    let encoded = serde_json::to_value(&native).unwrap();
    assert!(
        equal_json(&encoded, &expected),
        "{}: {encoded} != {expected}",
        row["id"]
    );
    observe(&native, expect);
}

fn run(row: &Value) {
    match row["target"].as_str().unwrap() {
        "example.weather#CitySummary" => round_trip::<CitySummary>(row, |native, expect| {
            if let Some(states) = expect["states"].as_object() {
                for (member, wanted) in states {
                    let actual = match member.as_str() {
                        "population" => "value",
                        "updatedAt" => state(&native.updated_at),
                        "createdAt" => state(&native.created_at),
                        _ => panic!("unknown observed member: {member}"),
                    };
                    assert_eq!(wanted, actual);
                }
            }
            if let Some(instant) = expect.get("instant") {
                let value = match instant["member"].as_str().unwrap() {
                    "updatedAt" => *native.updated_at.value().unwrap(),
                    "createdAt" => *native.created_at.value().unwrap(),
                    member => panic!("unknown timestamp member: {member}"),
                };
                assert_eq!(
                    value.timestamp().to_string(),
                    instant["seconds"].as_str().unwrap()
                );
                assert_eq!(
                    u64::from(value.timestamp_subsec_nanos()),
                    instant["nanoseconds"].as_u64().unwrap()
                );
            }
        }),
        "example.weather#Photo" => round_trip::<Photo>(row, |native, expect| {
            let hex: String = native
                .as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(hex, expect["blobHex"].as_str().unwrap());
        }),
        "example.weather#Sky" => round_trip::<Sky>(row, |_, _| {}),
        "example.weather#Location" => round_trip::<Location>(row, |_, _| {}),
        "example.weather#Forecast" => {
            // A collection alias is a native BTreeMap; its generated member
            // codec carries the model's nullability and enum-key semantics.
            let mut envelope = row.clone();
            let wrap = |wire: &str| {
                format!(
                    "{{\"city\":{{\"id\":\"c-test\",\"name\":\"test\",\"population\":0}},\"forecast\":{wire}}}"
                )
            };
            envelope["wire"] = wrap(row["wire"].as_str().unwrap()).into();
            if row["reject"] != true {
                let expected = row["expectByProfile"]
                    .get(PROFILE)
                    .unwrap_or(&row["expect"]);
                envelope["expect"]["json"] = wrap(expected["json"].as_str().unwrap()).into();
                envelope.as_object_mut().unwrap().remove("expectByProfile");
            }
            round_trip::<GetCityOutput>(&envelope, |_, _| {});
        }
        target => panic!("unknown corpus target: {target}"),
    }
}

fn state<T>(value: &Field<T>) -> &'static str {
    match value {
        Field::Absent => "absent",
        Field::Null => "null",
        Field::Value(_) => "value",
    }
}

#[test]
fn shared_serialization_all_16_cases() {
    let rows = cases();
    assert_eq!(rows.len(), 16, "Update corpus coverage when extending it");
    for row in &rows {
        run(row);
    }
}
