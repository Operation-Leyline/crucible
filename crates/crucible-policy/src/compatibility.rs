use crate::Severity;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Compatibility {
    pub severity: Severity,
    pub wire: bool,
    pub sdk_source: bool,
}

impl Default for Compatibility {
    fn default() -> Self {
        Self {
            severity: Severity::Error,
            wire: true,
            sdk_source: true,
        }
    }
}
