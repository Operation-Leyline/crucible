use crate::{Documentation, Naming, Paths};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rules {
    /// Empty selects the service's namespace. Imported contracts can opt in explicitly.
    pub namespaces: Vec<String>,
    pub naming: Option<Naming>,
    pub paths: Option<Paths>,
    pub documentation: Option<Documentation>,
}
