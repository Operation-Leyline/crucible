use crate::Severity;
use crate::check::{self, Check};
use serde::Deserialize;

/// Which consumers `diff` protects; each may carry its own severity.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Compatibility {
    pub severity: Severity,
    #[serde(deserialize_with = "check::enabled")]
    pub wire: Check<bool>,
    #[serde(deserialize_with = "check::enabled")]
    pub sdk_source: Check<bool>,
}

impl Default for Compatibility {
    fn default() -> Self {
        Self {
            severity: Severity::Error,
            wire: Check::new(true),
            sdk_source: Check::new(true),
        }
    }
}
