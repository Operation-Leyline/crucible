use crate::Severity;
use crate::check::{self, Check};
use serde::Deserialize;

/// What a reference needs beyond the types: an `example` on each operation,
/// and one showing each error the operation declares. A warning by default,
/// since a model without them is valid, only harder to call.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Examples {
    pub severity: Severity,
    /// Each operation declares at least one `example`.
    #[serde(deserialize_with = "check::enabled")]
    pub operations: Check<bool>,
    /// Each error an operation declares (not the service's shared ones)
    /// appears in one of its examples.
    #[serde(deserialize_with = "check::enabled")]
    pub errors: Check<bool>,
}

impl Default for Examples {
    fn default() -> Self {
        Self {
            severity: Severity::Warning,
            operations: Check::new(true),
            errors: Check::new(true),
        }
    }
}
