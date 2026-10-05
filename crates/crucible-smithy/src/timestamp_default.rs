//! Model timestamp defaults use epoch seconds or RFC 3339, independently of wire format.
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;

use crate::ir::TimestampFormat;

pub(crate) fn wire_value(value: &Value, format: TimestampFormat) -> Option<Value> {
    let time = match value {
        Value::Number(number) => from_decimal(&number.to_string())?,
        Value::String(text) => DateTime::parse_from_rfc3339(text).ok()?.with_timezone(&Utc),
        _ => return None,
    };
    Some(match format {
        TimestampFormat::DateTime => {
            Value::String(time.to_rfc3339_opts(SecondsFormat::AutoSi, true))
        }
        TimestampFormat::HttpDate => {
            Value::String(time.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
        }
        TimestampFormat::EpochSeconds if value.is_number() => value.clone(),
        TimestampFormat::EpochSeconds => {
            let nanos = i128::from(time.timestamp()) * 1_000_000_000
                + i128::from(time.timestamp_subsec_nanos());
            let sign = if nanos < 0 { "-" } else { "" };
            let magnitude = nanos.abs();
            let text = format!(
                "{sign}{}.{:09}",
                magnitude / 1_000_000_000,
                magnitude % 1_000_000_000
            );
            serde_json::from_str(text.trim_end_matches('0').trim_end_matches('.')).ok()?
        }
    })
}

fn from_decimal(text: &str) -> Option<DateTime<Utc>> {
    // Decimal arithmetic keeps fractional defaults exact, including pre-epoch instants.
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (text, 0),
    };
    let fraction = mantissa
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let digits = mantissa.replace('.', "").parse::<i128>().ok()?;
    let scale = exponent
        .checked_sub(i32::try_from(fraction).ok()?)?
        .checked_add(9)?;
    let nanos = if scale >= 0 {
        digits.checked_mul(10i128.checked_pow(scale as u32)?)?
    } else {
        // A divisor beyond i128 is larger than any parsed mantissa, so the
        // existing subnanosecond truncation yields zero rather than overflow.
        10i128
            .checked_pow(scale.unsigned_abs())
            .map_or(0, |divisor| digits / divisor)
    };
    DateTime::from_timestamp(
        i64::try_from(nanos.div_euclid(1_000_000_000)).ok()?,
        nanos.rem_euclid(1_000_000_000) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_format_is_independent_of_model_representation() {
        assert_eq!(
            wire_value(&json!(-0.25), TimestampFormat::DateTime),
            Some(json!("1969-12-31T23:59:59.750Z"))
        );
        assert_eq!(
            wire_value(&json!(1700000000.123), TimestampFormat::DateTime),
            Some(json!("2023-11-14T22:13:20.123Z"))
        );
        assert_eq!(
            wire_value(&json!(-0.25), TimestampFormat::HttpDate),
            Some(json!("Wed, 31 Dec 1969 23:59:59 GMT"))
        );
        assert_eq!(
            wire_value(
                &json!("1969-12-31T23:59:59.750Z"),
                TimestampFormat::EpochSeconds
            ),
            Some(json!(-0.25))
        );
        let precise: Value = serde_json::from_str("1700000000.123456789").unwrap();
        assert_eq!(
            wire_value(&precise, TimestampFormat::EpochSeconds),
            Some(precise)
        );
        assert_eq!(wire_value(&json!(1e300), TimestampFormat::DateTime), None);
    }

    #[test]
    fn subnanosecond_defaults_truncate_without_rejecting_tiny_magnitudes() {
        for value in [json!(1e-100), json!(-1e-100), json!(1e-10), json!(-1e-10)] {
            assert_eq!(
                wire_value(&value, TimestampFormat::DateTime),
                Some(json!("1970-01-01T00:00:00Z"))
            );
            assert_eq!(
                wire_value(&value, TimestampFormat::HttpDate),
                Some(json!("Thu, 01 Jan 1970 00:00:00 GMT"))
            );
            assert_eq!(
                wire_value(&value, TimestampFormat::EpochSeconds),
                Some(value)
            );
        }
    }
}
