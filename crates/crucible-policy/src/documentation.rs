use crate::{DocumentationTarget, Severity};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Documentation {
    pub severity: Severity,
    pub targets: Vec<DocumentationTarget>,
}
