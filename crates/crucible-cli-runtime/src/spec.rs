use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct Spec {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub env_prefix: String,
    pub endpoint: Option<String>,
    pub operations: Vec<Operation>,
    pub shapes: BTreeMap<String, Shape>,
}

#[derive(Deserialize)]
pub(crate) struct Operation {
    pub name: String,
    pub operation: String,
    #[serde(default)]
    pub description: String,
    pub input: Option<String>,
    #[serde(default)]
    pub paginated: bool,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Structure,
    Union,
    List,
    Map,
    String,
    Boolean,
    Integer,
    Number,
    Blob,
    Timestamp,
    Document,
    Unit,
}

#[derive(Deserialize)]
pub(crate) struct Shape {
    pub kind: Kind,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub members: Vec<Member>,
    pub target: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct Member {
    pub name: String,
    pub flag: String,
    pub target: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub sensitive: bool,
    pub kind: Option<Kind>,
}

impl Spec {
    pub fn shape(&self, id: &str) -> Result<&Shape, crate::Error> {
        self.shapes.get(id).ok_or_else(|| {
            crate::Error::input(format!("invalid command descriptor: missing shape {id}"))
        })
    }

    pub fn members(&self, operation: &Operation) -> Result<&[Member], crate::Error> {
        match &operation.input {
            Some(id) => Ok(&self.shape(id)?.members),
            None => Ok(&[]),
        }
    }
}
