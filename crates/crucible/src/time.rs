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

pub mod epoch_seconds {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn to_f64(time: &DateTime<Utc>) -> f64 {
        time.timestamp_millis() as f64 / 1000.0
    }

    pub fn from_f64(seconds: f64) -> Option<DateTime<Utc>> {
        DateTime::from_timestamp_millis((seconds * 1000.0).round() as i64)
    }

    pub fn serialize<S: Serializer>(
        time: &DateTime<Utc>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(to_f64(time))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<DateTime<Utc>, D::Error> {
        from_f64(f64::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("timestamp out of range"))
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
            match Option::<f64>::deserialize(deserializer)? {
                Some(seconds) => super::from_f64(seconds)
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
