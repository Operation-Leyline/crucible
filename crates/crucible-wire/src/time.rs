//! Timestamps. `date-time` (RFC 3339) is chrono's own serde form and needs
//! nothing here; the two formats `@timestampFormat` can choose instead have
//! serde modules generated members name in `#[serde(with = ...)]`.

use chrono::{DateTime, Utc};

pub type Timestamp = DateTime<Utc>;

pub fn format_date_time(time: &Timestamp) -> String {
    time.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}

pub fn parse_date_time(text: &str) -> Option<Timestamp> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

/// Timestamp text in `format`: what a label, query parameter or header holds.
pub fn parse(text: &str, format: crate::operation::TimestampFormat) -> Option<Timestamp> {
    use crate::operation::TimestampFormat;
    match format {
        TimestampFormat::DateTime => parse_date_time(text),
        TimestampFormat::EpochSeconds => epoch_seconds::parse(text),
        TimestampFormat::HttpDate => http_date::parse(text),
    }
}

/// A timestamp as text in `format`.
pub fn format(time: &Timestamp, format: crate::operation::TimestampFormat) -> String {
    use crate::operation::TimestampFormat;
    match format {
        TimestampFormat::DateTime => format_date_time(time),
        TimestampFormat::EpochSeconds => epoch_seconds::format(time),
        TimestampFormat::HttpDate => http_date::format(time),
    }
}

/// A timestamp's JSON value in any of its three formats: a number is epoch
/// seconds, and RFC 3339 and HTTP-date text cannot be mistaken for each other.
pub fn from_json(value: &serde_json::Value) -> Option<Timestamp> {
    match value {
        serde_json::Value::Number(number) => epoch_seconds::parse(&number.to_string()),
        serde_json::Value::String(text) => parse_date_time(text).or_else(|| http_date::parse(text)),
        _ => None,
    }
}

/// A timestamp as the JSON value of a member in `format`.
pub fn to_json(time: &Timestamp, format: crate::operation::TimestampFormat) -> serde_json::Value {
    use crate::operation::TimestampFormat;
    match format {
        TimestampFormat::EpochSeconds => serde_json::Value::Number(
            epoch_seconds::format(time)
                .parse()
                .expect("epoch seconds are JSON numeric syntax"),
        ),
        _ => serde_json::Value::String(self::format(time, format)),
    }
}

pub mod epoch_seconds {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    /// Exact decimal epoch seconds, including all nine fractional digits.
    pub fn format(time: &DateTime<Utc>) -> String {
        let nanos = i128::from(time.timestamp()) * 1_000_000_000
            + i128::from(time.timestamp_subsec_nanos());
        let magnitude = nanos.abs();
        let sign = if nanos < 0 { "-" } else { "" };
        let fraction = magnitude % 1_000_000_000;
        if fraction == 0 {
            format!("{sign}{}", magnitude / 1_000_000_000)
        } else {
            format!("{sign}{}.{fraction:09}", magnitude / 1_000_000_000)
                .trim_end_matches('0')
                .to_owned()
        }
    }

    /// Parses JSON numeric syntax without passing through a floating-point value.
    /// Values outside chrono's range or finer than a nanosecond are rejected.
    pub fn parse(text: &str) -> Option<DateTime<Utc>> {
        let number: serde_json::Number = text.parse().ok()?;
        let text = number.to_string();
        let (mantissa, exponent) = text.split_once(['e', 'E']).unwrap_or((&text, "0"));
        let exponent: i32 = exponent.parse().ok()?;
        let negative = mantissa.starts_with('-');
        let mantissa = mantissa.trim_start_matches('-');
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let digits = format!("{whole}{fraction}");
        let digits = digits.trim_start_matches('0');
        if digits.is_empty() {
            return DateTime::from_timestamp(0, 0);
        }
        let scale = exponent
            .checked_add(9)?
            .checked_sub(i32::try_from(fraction.len()).ok()?)?;
        let nanos = if scale < 0 {
            let removed = usize::try_from(scale.checked_neg()?).ok()?;
            let kept = digits.len().checked_sub(removed)?;
            if !digits[kept..].bytes().all(|c| c == b'0') {
                return None;
            }
            digits[..kept].parse::<i128>().ok()?
        } else {
            digits
                .parse::<i128>()
                .ok()?
                .checked_mul(10i128.checked_pow(scale.try_into().ok()?)?)?
        };
        let nanos = if negative { -nanos } else { nanos };
        DateTime::from_timestamp(
            nanos.div_euclid(1_000_000_000).try_into().ok()?,
            nanos.rem_euclid(1_000_000_000).try_into().ok()?,
        )
    }

    pub fn serialize<S: Serializer>(
        time: &DateTime<Utc>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        use serde::Serialize;
        format(time)
            .parse::<serde_json::Number>()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<DateTime<Utc>, D::Error> {
        let value = serde_json::Number::deserialize(deserializer)?;
        parse(&value.to_string()).ok_or_else(|| {
            serde::de::Error::custom("timestamp out of range or finer than nanoseconds")
        })
    }

    /// Presence-aware adapter used by generated optional client timestamps.
    pub mod field {
        use crate::field::Field;
        use chrono::{DateTime, Utc};
        use serde::{Deserializer, Serializer};
        pub fn serialize<S: Serializer>(
            time: &Field<DateTime<Utc>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match time {
                Field::Value(value) => super::serialize(value, serializer),
                Field::Absent | Field::Null => serializer.serialize_none(),
            }
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Field<DateTime<Utc>>, D::Error> {
            super::option::deserialize(deserializer)
                .map(|value| value.map_or(Field::Null, Field::Value))
        }
    }

    pub mod option {
        use chrono::{DateTime, Utc};
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(
            time: &Option<DateTime<Utc>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match time {
                Some(time) => super::serialize(time, serializer),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<DateTime<Utc>>, D::Error> {
            match Option::<serde_json::Number>::deserialize(deserializer)? {
                Some(seconds) => super::parse(&seconds.to_string())
                    .map(Some)
                    .ok_or_else(|| serde::de::Error::custom("timestamp out of range")),
                None => Ok(None),
            }
        }
    }
}

pub mod http_date {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn format(time: &DateTime<Utc>) -> String {
        time.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
    }

    pub fn parse(text: &str) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc2822(text)
            .ok()
            .map(|time| time.with_timezone(&Utc))
    }

    pub fn serialize<S: Serializer>(
        time: &DateTime<Utc>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format(time))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<DateTime<Utc>, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse(&text).ok_or_else(|| serde::de::Error::custom("not an HTTP date"))
    }

    /// Presence-aware adapter used by generated optional client timestamps.
    pub mod field {
        use crate::field::Field;
        use chrono::{DateTime, Utc};
        use serde::{Deserializer, Serializer};
        pub fn serialize<S: Serializer>(
            time: &Field<DateTime<Utc>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match time {
                Field::Value(value) => super::serialize(value, serializer),
                Field::Absent | Field::Null => serializer.serialize_none(),
            }
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Field<DateTime<Utc>>, D::Error> {
            super::option::deserialize(deserializer)
                .map(|value| value.map_or(Field::Null, Field::Value))
        }
    }

    pub mod option {
        use chrono::{DateTime, Utc};
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(
            time: &Option<DateTime<Utc>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match time {
                Some(time) => super::serialize(time, serializer),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<DateTime<Utc>>, D::Error> {
            match Option::<String>::deserialize(deserializer)? {
                Some(text) => super::parse(&text)
                    .map(Some)
                    .ok_or_else(|| serde::de::Error::custom("not an HTTP date")),
                None => Ok(None),
            }
        }
    }
}
