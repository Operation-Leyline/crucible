use serde::Serialize;

use crate::Severity;

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub rule: String,
    pub shape: String,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exception: Option<String>,
}
