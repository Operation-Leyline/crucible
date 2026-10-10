use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

use crate::{DocumentationTarget, NameCase, Severity};

/// One configured check: its value and, when it differs from its group's,
/// its own severity. Written as the bare value, `operations = "PascalCase"`,
/// or as a table naming the value, `members = { case = "camelCase", severity
/// = "warning" }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check<T> {
    pub value: T,
    pub severity: Option<Severity>,
}

impl<T> Check<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            severity: None,
        }
    }

    /// The check's own severity, else its group's.
    pub fn severity_in(&self, group: Severity) -> Severity {
        self.severity.unwrap_or(group)
    }
}

/// Reads a check whose table form names its value `key`. The config is TOML,
/// so the value is read as TOML first; a bare value is the check itself.
fn check<T: DeserializeOwned, E: serde::de::Error>(
    value: toml::Value,
    key: &str,
) -> Result<Check<T>, E> {
    match value {
        toml::Value::Table(mut table) => {
            let severity = table
                .remove("severity")
                .map(Severity::deserialize)
                .transpose()
                .map_err(E::custom)?;
            let value = table
                .remove(key)
                .ok_or_else(|| E::custom(format!("missing `{key}`")))?;
            if let Some(extra) = table.keys().next() {
                return Err(E::custom(format!(
                    "unknown field `{extra}`, expected `{key}` or `severity`"
                )));
            }
            Ok(Check {
                value: T::deserialize(value).map_err(E::custom)?,
                severity,
            })
        }
        bare => Ok(Check::new(T::deserialize(bare).map_err(E::custom)?)),
    }
}

/// `"PascalCase"` or `{ case = "PascalCase", severity = ".." }`.
pub(crate) fn case<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Check<NameCase>>, D::Error> {
    check(toml::Value::deserialize(d)?, "case").map(Some)
}

/// `"Get.*"` or `{ pattern = "Get.*", severity = ".." }`.
pub(crate) fn pattern<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Check<String>>, D::Error> {
    check(toml::Value::deserialize(d)?, "pattern").map(Some)
}

/// `"/v1"` or `{ path = "/v1", severity = ".." }`.
pub(crate) fn path<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Check<String>>, D::Error> {
    check(toml::Value::deserialize(d)?, "path").map(Some)
}

/// `true` or `{ enabled = true, severity = ".." }`.
pub(crate) fn enabled<'de, D: Deserializer<'de>>(d: D) -> Result<Check<bool>, D::Error> {
    check(toml::Value::deserialize(d)?, "enabled")
}

/// `["operations", { target = "errors", severity = "warning" }]`.
pub(crate) fn targets<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<Check<DocumentationTarget>>, D::Error> {
    Vec::<toml::Value>::deserialize(d)?
        .into_iter()
        .map(|value| check(value, "target"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Sample {
        #[serde(default, deserialize_with = "case")]
        bare: Option<Check<NameCase>>,
        #[serde(default, deserialize_with = "case")]
        table: Option<Check<NameCase>>,
    }

    #[test]
    fn a_check_is_a_bare_value_or_a_table_with_its_severity() {
        let sample: Sample = toml::from_str(
            "bare = 'PascalCase'\ntable = { case = 'camelCase', severity = 'warning' }",
        )
        .unwrap();
        assert_eq!(sample.bare, Some(Check::new(NameCase::PascalCase)));
        assert_eq!(
            sample.table,
            Some(Check {
                value: NameCase::CamelCase,
                severity: Some(Severity::Warning)
            })
        );
        assert_eq!(
            sample.table.unwrap().severity_in(Severity::Error),
            Severity::Warning
        );
        for text in [
            "table = { severity = 'warning' }",
            "table = { case = 'camelCase', level = 'warning' }",
            "table = { case = 'Camel' }",
            "table = { case = 'camelCase', severity = 'loud' }",
        ] {
            assert!(toml::from_str::<Sample>(text).is_err(), "{text}");
        }
    }
}
