use crate::{NameCase, Severity};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Paths {
    pub severity: Severity,
    pub prefix: Option<String>,
    pub segment_case: Option<NameCase>,
    pub parameter_case: Option<NameCase>,
}
