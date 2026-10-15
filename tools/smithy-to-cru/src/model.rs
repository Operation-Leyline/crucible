//! The Smithy JSON AST, read as plain JSON. The converter works from what the
//! author wrote, so it reads the file itself instead of the lowered IR.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

pub struct Smithy {
    pub shapes: BTreeMap<String, Value>,
}

impl Smithy {
    pub fn parse(json: &str) -> Result<Self, String> {
        let root: Value = serde_json::from_str(json).map_err(|e| format!("not JSON: {e}"))?;
        let shapes = root
            .get("shapes")
            .and_then(Value::as_object)
            .ok_or("not a Smithy JSON AST: no `shapes`")?;
        Ok(Self {
            shapes: shapes.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        })
    }
}

pub fn kind(shape: &Value) -> &str {
    shape.get("type").and_then(Value::as_str).unwrap_or("")
}

pub fn traits(shape: &Value) -> Option<&Map<String, Value>> {
    shape.get("traits").and_then(Value::as_object)
}

pub fn has_trait(shape: &Value, id: &str) -> bool {
    traits(shape).is_some_and(|t| t.contains_key(id))
}

pub fn get_trait<'a>(shape: &'a Value, id: &str) -> Option<&'a Value> {
    traits(shape).and_then(|t| t.get(id))
}

pub fn members(shape: &Value) -> Vec<(&String, &Value)> {
    shape
        .get("members")
        .and_then(Value::as_object)
        .map(|m| m.iter().collect())
        .unwrap_or_default()
}

pub fn target(member: &Value) -> &str {
    member.get("target").and_then(Value::as_str).unwrap_or("")
}

/// The ids in a list of `{ "target": id }` references.
pub fn refs(shape: &Value, field: &str) -> Vec<String> {
    shape
        .get(field)
        .and_then(Value::as_array)
        .map(|items| items.iter().map(|r| target(r).to_owned()).collect())
        .unwrap_or_default()
}

pub fn single_ref(shape: &Value, field: &str) -> Option<String> {
    shape.get(field).map(|r| target(r).to_owned())
}

pub fn short(id: &str) -> &str {
    id.split_once('#').map_or(id, |(_, name)| name)
}

pub fn namespace(id: &str) -> &str {
    id.split_once('#').map_or("", |(ns, _)| ns)
}

pub fn doc_of(traits: Option<&Map<String, Value>>) -> Option<&str> {
    traits
        .and_then(|t| t.get("smithy.api#documentation"))
        .and_then(Value::as_str)
}
