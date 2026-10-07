use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocumentationTarget {
    Service,
    Operations,
    Shapes,
    Members,
    Errors,
    Enums,
}
