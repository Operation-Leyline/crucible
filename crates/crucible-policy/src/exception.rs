use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    pub rule: String,
    pub shape: String,
    pub reason: String,
}
