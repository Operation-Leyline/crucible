use crate::Severity;
use serde::Deserialize;

/// Opt-in guidance coverage. Missing intent is a warning unless configured
/// otherwise, so projects can adopt the documentation convention incrementally.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Intent {
    pub severity: Severity,
}

impl Default for Intent {
    fn default() -> Self {
        Self {
            severity: Severity::Warning,
        }
    }
}
