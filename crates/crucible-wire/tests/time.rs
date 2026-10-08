use crucible_wire::{bound::BoundValue, operation::Scalar, time};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct HttpDate {
    #[serde(with = "time::http_date")]
    time: time::Timestamp,
}

#[test]
fn http_date_accepts_only_imf_fixdate() {
    let canonical = "Tue, 29 Apr 2014 18:30:38 GMT";
    let instant = time::http_date::parse(canonical).unwrap();
    assert_eq!(time::http_date::format(&instant), canonical);
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
