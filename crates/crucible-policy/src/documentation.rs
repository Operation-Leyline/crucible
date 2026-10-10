use crate::check::{self, Check};
use crate::{DocumentationTarget, Severity};
use serde::Deserialize;

/// What must be documented; each target may carry its own severity.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Documentation {
    pub severity: Severity,
    #[serde(deserialize_with = "check::targets")]
    pub targets: Vec<Check<DocumentationTarget>>,
}
