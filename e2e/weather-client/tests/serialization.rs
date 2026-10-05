//! Baseline coverage of the shared corpus. The four ignored contract cases name
//! existing public model limitations; they remain failing tests when opted in.
use crucible_client::__private::serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use weather_client::model::{CitySummary, Forecast, Location, Photo, Sky};

const PROFILE: &str = "rust-chrono-epoch-ms";
const GAPS: [&str; 4] = [
    "explicit-optional-null",
    "open-enum",
    "unknown-union-exact-document",
    "map-document-container-identity",
];

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
                for (member, state) in states {
                    let present = match member.as_str() {
                        "population" => true,
                        "updatedAt" => native.updated_at.is_some(),
                        "createdAt" => native.created_at.is_some(),
                        _ => panic!("unknown observed member: {member}"),
                    };
                    assert_eq!(state, if present { "value" } else { "absent" });
                }
            }
            if let Some(instant) = expect.get("instant") {
                let value = match instant["member"].as_str().unwrap() {
                    "updatedAt" => native.updated_at.unwrap(),
                    "createdAt" => native.created_at.unwrap(),
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
        "example.weather#Forecast" => round_trip::<Forecast>(row, |_, _| {}),
        target => panic!("unknown corpus target: {target}"),
    }
}

#[test]
fn shared_serialization_supported_cases() {
    let rows = cases();
    assert_eq!(
        rows.len(),
        15,
        "Update baseline coverage when extending the corpus"
    );
    for gap in GAPS {
        assert!(rows.iter().any(|row| row["id"] == gap));
    }
    let mut supported = 0;
    for row in &rows {
        if !GAPS.contains(&row["id"].as_str().unwrap()) {
            run(row);
            supported += 1;
        }
    }
    assert_eq!(supported, 11);
}

fn contract_case(id: &str) {
    run(&cases().into_iter().find(|row| row["id"] == id).unwrap());
}

#[test]
#[ignore = "Rust Option conflates absent and explicit null; requires a public presence type"]
fn explicit_optional_null() {
    contract_case("explicit-optional-null");
}

#[test]
#[ignore = "Generated Rust enums are closed and cannot preserve unknown wire values"]
fn open_enum() {
    contract_case("open-enum");
}

#[test]
#[ignore = "Generated Rust unions are closed and cannot preserve unknown members"]
fn unknown_union_exact_document() {
    contract_case("unknown-union-exact-document");
}

#[test]
#[ignore = "Forecast uses the closed Sky enum as keys and rejects the future key"]
fn map_document_container_identity() {
    contract_case("map-document-container-identity");
}
