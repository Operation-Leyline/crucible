use crate::check::{self, Check};
use crate::{NameCase, Severity};
use serde::Deserialize;

/// Naming conventions. Each check is a case or a whole-name regex, bare or
/// with its own severity; the group's `severity` is the default.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Naming {
    pub severity: Severity,
    #[serde(deserialize_with = "check::case")]
    pub shapes: Option<Check<NameCase>>,
    #[serde(deserialize_with = "check::case")]
    pub operations: Option<Check<NameCase>>,
    #[serde(deserialize_with = "check::case")]
    pub members: Option<Check<NameCase>>,
    #[serde(deserialize_with = "check::case")]
    pub enum_values: Option<Check<NameCase>>,
    #[serde(deserialize_with = "check::pattern")]
    pub operations_pattern: Option<Check<String>>,
    #[serde(deserialize_with = "check::pattern")]
    pub shapes_pattern: Option<Check<String>>,
    #[serde(deserialize_with = "check::pattern")]
    pub members_pattern: Option<Check<String>>,
    #[serde(deserialize_with = "check::pattern")]
    pub enum_values_pattern: Option<Check<String>>,
}
