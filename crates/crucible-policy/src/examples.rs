use crate::Severity;
use crate::check::{self, Check};
use serde::Deserialize;

/// What a reference needs beyond the types: an `example` on each operation,
/// and documentation on each error an operation can return. A warning by
/// default, since a model without them is valid, only harder to call.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Examples {
    pub severity: Severity,
    /// Each operation declares at least one `example`.
    #[serde(deserialize_with = "check::enabled")]
    pub operations: Check<bool>,
    /// Each modelled error an operation returns, its own or the service's,
    /// is documented.
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
