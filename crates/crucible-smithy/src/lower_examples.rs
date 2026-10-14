//! `@examples`: lowered with the operation and checked against the lowered
//! input, output and error shapes, so an example is held to the same member,
//! type and constraint rules as a value on the wire.

use std::collections::BTreeMap;

use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Map, Number, Value};

use crate::ast;
use crate::ir::{
    Constraints, Example, ExampleError, Member, Service, Shape, Simple, SimpleKind, Structure,
    TimestampFormat,
};
use crate::lower::Lowerer;
use crate::shape_id::ShapeId;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    title: String,
    documentation: Option<String>,
    input: Option<Value>,
    output: Option<Value>,
    error: Option<ErrorEntry>,
    #[serde(default)]
    allow_constraint_errors: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ErrorEntry {
    shape_id: String,
    #[serde(default = "empty_object")]
    content: Value,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

impl Lowerer<'_> {
    pub(crate) fn examples(
        &mut self,
        id: &ShapeId,
        operation: &ast::Shape,
        errors: &[ShapeId],
    ) -> Vec<Example> {
        let Some(value) = operation
            .traits
            .get(&ShapeId::new("smithy.api", "examples"))
        else {
            return Vec::new();
        };
        let Some(entries) = value.as_array() else {
            self.diagnostics.push(id, "@examples must be a list");
            return Vec::new();
        };
        let mut examples = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let entry: Entry = match serde_json::from_value(entry.clone()) {
                Ok(entry) => entry,
                Err(error) => {
                    self.diagnostics
                        .push(id, format!("@examples[{index}] is malformed: {error}"));
                    continue;
                }
            };
            // Values are checked against the lowered shapes by
            // crate::validate, shared with the .cru front end.
            let mut error = None;
            if let Some(entry_error) = &entry.error {
                match entry_error.shape_id.parse::<ShapeId>() {
                    Ok(shape) if errors.contains(&shape) => {
                        error = Some(ExampleError {
                            shape,
                            content: entry_error.content.clone(),
                        });
                    }
                    _ => self.diagnostics.push(
                        id,
                        format!(
                            "@examples[{index}] {:?}: error.shapeId {:?} is not an error of this operation",
                            entry.title, entry_error.shape_id
                        ),
                    ),
                }
            }
            examples.push(Example {
                title: entry.title,
                docs: entry.documentation,
                input: entry.input,
                output: entry.output,
                error,
                allow_constraint_errors: entry.allow_constraint_errors,
            });
        }
        examples
    }
}

/// Checks model values against lowered shapes: examples and defaults.
pub(crate) struct Check<'a> {
    pub(crate) shapes: &'a BTreeMap<ShapeId, Shape>,
    pub(crate) allow_constraint_errors: bool,
    pub(crate) problems: Vec<String>,
}

impl Check<'_> {
    /// An operation's input, output or error, where no shape means Unit.
    pub(crate) fn io(&mut self, path: &str, shape: Option<&ShapeId>, value: &Value) {
        let Value::Object(members) = value else {
            self.problems
                .push(format!("{path} must be an object, not {}", kind(value)));
            return;
        };
        match shape.and_then(|shape| self.shapes.get(shape)) {
            Some(Shape::Structure(structure)) => self.structure(path, structure, members),
            _ if members.is_empty() => {}
            _ => self
                .problems
                .push(format!("{path} must be empty: the operation declares none")),
        }
    }

    /// Violations `allowConstraintErrors` permits.
    fn constraint(&mut self, problem: String) {
        if !self.allow_constraint_errors {
            self.problems.push(problem);
        }
    }

    fn structure(&mut self, path: &str, shape: &Structure, members: &Map<String, Value>) {
        for (name, value) in members {
            match shape.member(name) {
                // A document's null is a value of its own, distinct on the
                // wire from leaving the member out, and Smithy accepts null
                // for a nullable member. A required one stays refused.
                Some(member)
                    if value.is_null()
                        && member.optional_on_server()
                        && self.is_document(&member.target) => {}
                Some(member) => self.member(&format!("{path}.{name}"), member, value, false),
                None => self.problems.push(format!("{path}.{name} is not a member")),
            }
        }
        for member in &shape.members {
            if member.required && member.default.is_none() && !members.contains_key(&member.name) {
                self.constraint(format!("{path}.{} is required", member.name));
            }
        }
    }

    fn is_document(&self, target: &ShapeId) -> bool {
        matches!(
            self.shapes.get(target),
            Some(Shape::Simple(simple)) if simple.kind == SimpleKind::Document
        )
    }

    fn member(&mut self, path: &str, member: &Member, value: &Value, sparse: bool) {
        if value.is_null() {
            if !sparse {
                self.problems.push(format!("{path} must not be null"));
            }
            return;
        }
        self.value(path, &member.target, &member.constraints, value);
    }

    pub(crate) fn value(
        &mut self,
        path: &str,
        target: &ShapeId,
        constraints: &Constraints,
        value: &Value,
    ) {
        let Some(shape) = self.shapes.get(target) else {
            return;
        };
        match shape {
            Shape::Unit { .. } => {
                if value.as_object().is_none_or(|o| !o.is_empty()) {
                    self.problems
                        .push(format!("{path} must be an empty object"));
                }
            }
            Shape::Simple(simple) => self.simple(path, simple, constraints, value),
            Shape::Enum(enumeration) => match value.as_str() {
                Some(text) if enumeration.values.iter().any(|v| v.value == text) => {}
                Some(text) => self
                    .problems
                    .push(format!("{path}: {text:?} is not a value of {target}")),
                None => self.mismatch(path, "string", value),
            },
            Shape::IntEnum(enumeration) => match value.as_i64() {
                Some(n) if enumeration.values.iter().any(|v| i64::from(v.value) == n) => {}
                Some(n) => self
                    .problems
                    .push(format!("{path}: {n} is not a value of {target}")),
                None => self.mismatch(path, "integer", value),
            },
            Shape::List(list) => {
                let Some(items) = value.as_array() else {
                    return self.mismatch(path, "array", value);
                };
                self.length(path, constraints, items.len() as u64, "items");
                if (list.is_set || constraints.unique_items)
                    && items
                        .iter()
                        .enumerate()
                        .any(|(i, item)| items[..i].contains(item))
                {
                    self.constraint(format!("{path} must not repeat items"));
                }
                for (index, item) in items.iter().enumerate() {
                    self.member(&format!("{path}[{index}]"), &list.member, item, list.sparse);
                }
            }
            Shape::Map(map) => {
                let Some(entries) = value.as_object() else {
                    return self.mismatch(path, "object", value);
                };
                self.length(path, constraints, entries.len() as u64, "entries");
                for (key, entry) in entries {
                    self.value(
                        &format!("{path} key {key:?}"),
                        &map.key.target,
                        &map.key.constraints,
                        &Value::String(key.clone()),
                    );
                    self.member(&format!("{path}.{key}"), &map.value, entry, map.sparse);
                }
            }
            Shape::Structure(structure) => match value.as_object() {
                Some(members) => self.structure(path, structure, members),
                None => self.mismatch(path, "object", value),
            },
            Shape::Union(union) => {
                let Some(members) = value.as_object() else {
                    return self.mismatch(path, "object", value);
                };
                if members.len() != 1 {
                    self.problems
                        .push(format!("{path} must set exactly one union member"));
                }
                for (name, value) in members {
                    match union.member(name) {
                        Some(member) => {
                            self.member(&format!("{path}.{name}"), member, value, false)
                        }
                        None => self.problems.push(format!("{path}.{name} is not a member")),
                    }
                }
            }
        }
    }

    fn simple(&mut self, path: &str, simple: &Simple, constraints: &Constraints, value: &Value) {
        match simple.kind {
            SimpleKind::Document => {}
            SimpleKind::Boolean if value.is_boolean() => {}
            SimpleKind::Boolean => self.mismatch(path, "boolean", value),
            SimpleKind::String => {
                let Some(text) = value.as_str() else {
                    return self.mismatch(path, "string", value);
                };
                self.length(path, constraints, text.chars().count() as u64, "characters");
                // A pattern the regex engine cannot run is diagnosed where the
                // pattern is used, so only this check is skipped.
                if let Ok(Some(pattern)) = constraints.rust_pattern()
                    && let Ok(pattern) = regex::Regex::new(&pattern)
                    && !pattern.is_match(text)
                {
                    self.constraint(format!(
                        "{path} does not match the pattern {:?}",
                        constraints.pattern.as_deref().unwrap_or_default()
                    ));
                }
            }
            SimpleKind::Blob => {
                let Some(text) = value.as_str() else {
                    return self.mismatch(path, "base64 string", value);
                };
                match base64::engine::general_purpose::STANDARD.decode(text) {
                    Ok(bytes) => self.length(path, constraints, bytes.len() as u64, "bytes"),
                    Err(_) => self.problems.push(format!("{path} must be valid base64")),
                }
            }
            SimpleKind::Timestamp => {
                let valid = matches!(value, Value::Number(_) | Value::String(_))
                    && crate::timestamp_default::wire_value(value, TimestampFormat::DateTime)
                        .is_some();
                if !valid {
                    self.problems.push(format!(
                        "{path} must be an RFC 3339 string or epoch seconds"
                    ));
                }
            }
            SimpleKind::Float | SimpleKind::Double => match value {
                Value::Number(number) => self.range(path, constraints, number),
                Value::String(text)
                    if matches!(text.as_str(), "NaN" | "Infinity" | "-Infinity") => {}
                _ => self.mismatch(path, "number", value),
            },
            SimpleKind::BigDecimal => match value {
                Value::Number(number) => self.range(path, constraints, number),
                _ => self.mismatch(path, "number", value),
            },
            SimpleKind::BigInteger => match value {
                Value::Number(number) if !number.to_string().contains(['.', 'e', 'E']) => {
                    self.range(path, constraints, number)
                }
                _ => self.mismatch(path, "integer", value),
            },
            SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long => {
                let (low, high) = match simple.kind {
                    SimpleKind::Byte => (i128::from(i8::MIN), i128::from(i8::MAX)),
                    SimpleKind::Short => (i128::from(i16::MIN), i128::from(i16::MAX)),
                    SimpleKind::Integer => (i128::from(i32::MIN), i128::from(i32::MAX)),
                    _ => (i128::from(i64::MIN), i128::from(i64::MAX)),
                };
                let Value::Number(number) = value else {
                    return self.mismatch(path, "integer", value);
                };
                match number.to_string().parse::<i128>() {
                    Ok(n) if (low..=high).contains(&n) => self.range(path, constraints, number),
                    Ok(n) => self.problems.push(format!(
                        "{path}: {n} is outside the range of the type ({low} to {high})"
                    )),
                    Err(_) => self.mismatch(path, "integer", value),
                }
            }
        }
    }

    fn length(&mut self, path: &str, constraints: &Constraints, actual: u64, unit: &str) {
        let Some(bounds) = &constraints.length else {
            return;
        };
        if bounds.min.is_some_and(|min| actual < min) || bounds.max.is_some_and(|max| actual > max)
        {
            self.constraint(format!(
                "{path} has {actual} {unit}, outside its length bounds ({}..{})",
                bounds.min.map_or(String::new(), |n| n.to_string()),
                bounds.max.map_or(String::new(), |n| n.to_string())
            ));
        }
    }

    fn range(&mut self, path: &str, constraints: &Constraints, number: &Number) {
        let Some(bounds) = &constraints.range else {
            return;
        };
        let below = bounds.min.as_ref().is_some_and(|min| precedes(number, min));
        let above = bounds.max.as_ref().is_some_and(|max| precedes(max, number));
        if below || above {
            self.constraint(format!("{path}: {number} is outside its range"));
        }
    }

    fn mismatch(&mut self, path: &str, expected: &str, value: &Value) {
        self.problems
            .push(format!("{path} must be a {expected}, not {}", kind(value)));
    }
}

/// Whether `a < b`. Integers compare exactly, as `f64` would round `Long` and
/// `BigInteger` values past 2^53 onto their bounds.
fn precedes(a: &Number, b: &Number) -> bool {
    match (a.to_string().parse::<i128>(), b.to_string().parse::<i128>()) {
        (Ok(a), Ok(b)) => a < b,
        _ => matches!((a.as_f64(), b.as_f64()), (Some(a), Some(b)) if a < b),
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

impl Service {
    /// A validated example value for `member`, as a document carries it: JSON
    /// names for members and the member's timestamp format. `format` is the
    /// HTTP binding's timestamp format, which overrides it at the top level.
    pub fn wire_example(
        &self,
        member: &Member,
        value: &Value,
        format: Option<TimestampFormat>,
    ) -> Value {
        let Some(shape) = self.shape(&member.target) else {
            return value.clone();
        };
        match (shape, value) {
            (Shape::Simple(simple), Value::Number(number))
                if self.protocol_settings.arbitrary_precision_strings
                    && matches!(simple.kind, SimpleKind::BigInteger | SimpleKind::BigDecimal) =>
            {
                Value::String(number.to_string())
            }
            (Shape::Simple(simple), _) if simple.kind == SimpleKind::Timestamp => {
                let format = format.unwrap_or_else(|| self.document_timestamp_format(member));
                crate::timestamp_default::wire_value(value, format).unwrap_or_else(|| value.clone())
            }
            (Shape::List(list), Value::Array(items)) => Value::Array(
                items
                    .iter()
                    .map(|item| self.wire_example(&list.member, item, format))
                    .collect(),
            ),
            (Shape::Map(map), Value::Object(entries)) => Value::Object(
                entries
                    .iter()
                    .map(|(key, entry)| (key.clone(), self.wire_example(&map.value, entry, None)))
                    .collect(),
            ),
            (Shape::Structure(structure) | Shape::Union(structure), Value::Object(members)) => {
                Value::Object(
                    members
                        .iter()
                        .filter(|(_, value)| {
                            !(matches!(shape, Shape::Structure(_))
                                && value.is_null()
                                && self.protocol_settings.omit_null_structure_members)
                        })
                        .map(|(name, value)| match structure.member(name) {
                            Some(member) => (
                                member.wire_name.clone(),
                                self.wire_example(member, value, None),
                            ),
                            None => (name.clone(), value.clone()),
                        })
                        .collect(),
                )
            }
            _ => value.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_bounds_compare_exactly_past_f64_precision() {
        let bound: Number = "9007199254740993".parse().unwrap();
        let below: Number = "9007199254740992".parse().unwrap();
        assert!(precedes(&below, &bound));
        assert!(!precedes(&bound, &below));
        assert!(!precedes(&bound, &bound));
        let fraction: Number = "1.5".parse().unwrap();
        let two: Number = "2".parse().unwrap();
        assert!(precedes(&fraction, &two));
    }
}
