use crate::{NameCase, Severity};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Naming {
    pub severity: Severity,
    pub shapes: Option<NameCase>,
    pub operations: Option<NameCase>,
    pub members: Option<NameCase>,
    pub enum_values: Option<NameCase>,
    pub operations_pattern: Option<String>,
    pub shapes_pattern: Option<String>,
    pub members_pattern: Option<String>,
    pub enum_values_pattern: Option<String>,
}
