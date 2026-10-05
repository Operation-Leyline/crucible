use chrono::{DateTime, Utc};
use crucible_wire::{
    bound::{self, BoundValue},
    field::Field,
    operation::Scalar,
    time,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Values {
    #[serde(with = "time::epoch_seconds")]
    instant: DateTime<Utc>,
    #[serde(default, with = "time::epoch_seconds::option")]
    optional: Option<DateTime<Utc>>,
    #[serde(default, with = "time::epoch_seconds::field")]
    presence: Field<DateTime<Utc>>,
}

#[test]
fn nanoseconds_roundtrip_in_json_and_bound_headers_without_floats() {
    for text in [
        "1790812800.123456789",
        "-0.000000001",
        "-1.123456789",
        "1790812800123456789e-9",
        "253402300799.999999999",
    ] {
        let time = time::epoch_seconds::parse(text).unwrap();
        let wire = format!("{{\"instant\":{text},\"optional\":{text},\"presence\":{text}}}");
        let expected = Values {
            instant: time,
            optional: Some(time),
            presence: Field::Value(time),
        };
        assert_eq!(serde_json::from_str::<Values>(&wire).unwrap(), expected);
        let written = serde_json::to_string(&expected).unwrap();
        assert_eq!(serde_json::from_str::<Values>(&written).unwrap(), expected);
        assert!(written.contains(&time::epoch_seconds::format(&time)));
        let bound = ["instant", "optional", "presence"]
            .into_iter()
            .map(|name| {
                (
                    name,
                    BoundValue::parse(text.into(), Scalar::EpochSeconds).unwrap(),
                )
            })
            .collect();
        assert_eq!(bound::read::<Values>(b"{}", bound).unwrap(), expected);
    }
}

#[test]
fn subnanosecond_and_out_of_range_numbers_are_rejected() {
    for text in [
        "1e300",
        "0.0000000001",
        "-99999999999999999999999999",
        "NaN",
        "null",
    ] {
        assert!(time::epoch_seconds::parse(text).is_none(), "{text}");
    }
    assert_eq!(
        time::epoch_seconds::format(&time::epoch_seconds::parse("-0.2500000000").unwrap()),
        "-0.25"
    );
}
