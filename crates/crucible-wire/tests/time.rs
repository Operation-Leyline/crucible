use crucible_wire::{
    bound::BoundValue,
    operation::{Location, MemberInfo, Redaction, Scalar, TimestampFormat},
    time,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct HttpDate {
    #[serde(with = "time::http_date")]
    time: time::Timestamp,
}

#[test]
fn timestamp_text_conversion_rejects_years_outside_four_digits() {
    for text in ["253402300800", "-62167219201"] {
        let instant = time::epoch_seconds::parse(text).unwrap();
        assert!(time::format_date_time(&instant).is_err());
        assert!(time::http_date::format(&instant).is_err());
        assert!(serde_json::to_string(&HttpDate { time: instant }).is_err());
        for json in [TimestampFormat::DateTime, TimestampFormat::HttpDate] {
            assert!(time::to_json(&instant, json).is_err());
            let value = serde_json::from_str(text).unwrap();
            assert!(
                crucible_wire::text::scalar_text(
                    &value,
                    Scalar::Timestamp {
                        text: json,
                        json: TimestampFormat::EpochSeconds,
                    }
                )
                .is_err()
            );
            assert!(
                BoundValue::parse(
                    text.into(),
                    Scalar::Timestamp {
                        text: TimestampFormat::EpochSeconds,
                        json,
                    }
                )
                .is_err(),
                "{text}: {json:?}"
            );
        }
    }
}

#[test]
fn timestamp_text_roundtrips_the_four_digit_year_boundaries() {
    for text in ["0000-01-01T00:00:00Z", "9999-12-31T23:59:59Z"] {
        let instant = time::parse_date_time(text).unwrap();
        assert_eq!(time::format_date_time(&instant).unwrap(), text);
        let http = time::http_date::format(&instant).unwrap();
        assert_eq!(time::http_date::parse(&http), Some(instant));
        assert_eq!(
            time::epoch_seconds::parse(&time::epoch_seconds::format(&instant)),
            Some(instant)
        );
        for format in [
            TimestampFormat::DateTime,
            TimestampFormat::HttpDate,
            TimestampFormat::EpochSeconds,
        ] {
            assert_eq!(
                time::from_json(&time::to_json(&instant, format).unwrap()),
                Some(instant)
            );
        }
    }
}

#[test]
fn date_time_decode_rejects_offsets_crossing_the_year_limits() {
    for text in ["0000-01-01T00:00:00+00:01", "9999-12-31T23:59:59-00:01"] {
        assert!(time::parse_date_time(text).is_none(), "{text}");
    }
}

#[test]
fn response_headers_reject_out_of_range_timestamp_text() {
    #[derive(Serialize)]
    struct Output {
        #[serde(with = "time::epoch_seconds")]
        time: time::Timestamp,
    }
    for text in ["253402300800", "-62167219201"] {
        let output = Output {
            time: time::epoch_seconds::parse(text).unwrap(),
        };
        for format in [TimestampFormat::DateTime, TimestampFormat::HttpDate] {
            let members = [MemberInfo {
                name: "time",
                location: Location::Header("x-time"),
                scalar: Scalar::Timestamp {
                    text: format,
                    json: TimestampFormat::EpochSeconds,
                },
                redaction: &Redaction::Public,
                traits: &[],
            }];
            assert!(crucible_wire::split::write(&output, &members, &mut Vec::new()).is_err());
            assert!(
                crucible_wire::split::write_http(&output, &members, None, &mut Vec::new()).is_err()
            );
        }
    }
}

#[test]
fn http_date_accepts_only_imf_fixdate() {
    let canonical = "Tue, 29 Apr 2014 18:30:38 GMT";
    let instant = time::http_date::parse(canonical).unwrap();
    assert_eq!(time::http_date::format(&instant).unwrap(), canonical);
    for text in [
        "29 Apr 2014 18:30:38 GMT",
        "Tue, 29 Apr 2014 18:30:38 +0100",
        "Tue, 29 Apr 2014 18:30:38 +0000",
        "Tue, 29 Apr 2014 18:30:38 EST",
        "Tue, 29 Apr 2014 18:30:38 Z",
        "Tue, 29 Apr 14 18:30:38 GMT",
        "Tuesday, 29-Apr-14 18:30:38 GMT",
        "Tue Apr 29 18:30:38 2014",
        "tue, 29 Apr 2014 18:30:38 GMT",
        "Tue, 29 apr 2014 18:30:38 GMT",
        "Tue, 29 Apr 2014 18:30:38 gmt",
        "Tue,  29 Apr 2014 18:30:38 GMT",
        "Tue, 29 Apr 2014 18:30:38 GMT ",
        "Wed, 29 Apr 2014 18:30:38 GMT",
        "Tue, 29 Apr 2014 18:30:38 GMT (comment)",
        "Tue, 29 Apr 2014 18:30:38.123 GMT",
    ] {
        assert!(time::http_date::parse(text).is_none(), "{text}");
        assert!(
            BoundValue::parse(text.into(), Scalar::HttpDate).is_err(),
            "{text}"
        );
        let json = serde_json::json!({"time": text});
        assert!(serde_json::from_value::<HttpDate>(json).is_err(), "{text}");
    }
}
