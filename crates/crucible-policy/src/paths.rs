use crate::check::{self, Check};
use crate::{NameCase, Severity};
use serde::Deserialize;

/// URI conventions; each check may carry its own severity.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Paths {
    pub severity: Severity,
    #[serde(deserialize_with = "check::path")]
    pub prefix: Option<Check<String>>,
    #[serde(deserialize_with = "check::case")]
    pub segment_case: Option<Check<NameCase>>,
    #[serde(deserialize_with = "check::case")]
    pub parameter_case: Option<Check<NameCase>>,
}
