//! Model literals read against the shape they are a value of: an unquoted
//! identifier where an enum is expected names a symbol, at any depth.

use std::collections::{BTreeMap, BTreeSet};

use crucible_smithy::ShapeId;
use crucible_smithy::ir::Shape;
use serde_json::Value as Json;

use crate::ast::{Key, Value};

/// Rewrites each enum symbol in `json`, the plain conversion of `value`, to
/// its wire value, following `target` through structures, unions, lists and
/// maps. A quoted string is already a wire value and is left alone, even when
/// it spells another symbol's name, so `default "RED"` and `default RED`
/// differ when `SPECIAL = "RED"`. A map key is text, except an unquoted key
/// of a map keyed by an enum, which names a symbol as a value would:
/// `{ RED: "x" }` is keyed by RED's wire value, `{ "RED": "x" }` by `RED`.
///
/// Returns a problem for each map whose keys collide once resolved
/// (`{ RED: "x", "r": "y" }`), which would otherwise drop a value unseen.
pub(crate) fn resolve_enum_symbols(
    shapes: &BTreeMap<ShapeId, Shape>,
    target: &ShapeId,
    value: &Value,
    json: &mut Json,
) -> Vec<String> {
    let mut problems = Vec::new();
    resolve(shapes, target, value, json, &mut problems);
    problems
}

fn resolve(
    shapes: &BTreeMap<ShapeId, Shape>,
    target: &ShapeId,
    value: &Value,
    json: &mut Json,
    problems: &mut Vec<String>,
) {
    let Some(shape) = shapes.get(target) else {
        return;
    };
    match (shape, value, json) {
        (Shape::Enum(enumeration), Value::Ident(symbol), json) => {
            if let Some(found) = enumeration.values.iter().find(|v| v.name == symbol.name) {
                *json = Json::String(found.value.clone());
            }
        }
        (Shape::IntEnum(enumeration), Value::Ident(symbol), json) => {
            if let Some(found) = enumeration.values.iter().find(|v| v.name == symbol.name) {
                *json = Json::Number(found.value.into());
            }
        }
        (Shape::List(list), Value::List(items, _), Json::Array(array)) => {
            for (item, json) in items.iter().zip(array.iter_mut()) {
                resolve(shapes, &list.member.target, item, json, problems);
            }
        }
        (Shape::Map(map), Value::Object(fields, _), Json::Object(object)) => {
            let symbols = match shapes.get(&map.key.target) {
                Some(Shape::Enum(enumeration)) => Some(enumeration),
                _ => None,
            };
            // The conversion keeps a key's last value; its last field says
            // whether the key was a symbol.
            let mut entries = Vec::new();
            for (key, mut json) in std::mem::take(object) {
                let Some(index) = fields.iter().rposition(|f| key_text(&f.key) == key) else {
                    entries.push((0, key, json));
                    continue;
                };
                let field = &fields[index];
                resolve(shapes, &map.value.target, &field.value, &mut json, problems);
                let key = match (&field.key, symbols) {
                    (Key::Name(name), Some(symbols)) => symbols
                        .values
                        .iter()
                        .find(|v| v.name == name.name)
                        .map_or(key, |v| v.value.clone()),
                    _ => key,
                };
                entries.push((index, key, json));
            }
            entries.sort_by_key(|(index, _, _)| *index);
            for (_, key, json) in entries {
                // A symbol and another key may name one wire value.
                if object.insert(key.clone(), json).is_some() {
                    problems.push(format!("`{key}` is given twice"));
                }
            }
        }
        (Shape::Structure(_) | Shape::Union(_), Value::Object(fields, _), Json::Object(object)) => {
            // A key given twice keeps its last value, as the conversion does.
            let mut seen = BTreeSet::new();
            for field in fields.iter().rev() {
                let key = key_text(&field.key);
                if !seen.insert(key) {
                    continue;
                }
                let target = match shape {
                    Shape::Structure(s) | Shape::Union(s) => s.member(key).map(|m| &m.target),
                    _ => None,
                };
                if let (Some(target), Some(json)) = (target, object.get_mut(key)) {
                    resolve(shapes, target, &field.value, json, problems);
                }
            }
        }
        _ => {}
    }
}

/// A key as the plain conversion writes it.
fn key_text(key: &Key) -> &str {
    match key {
        Key::Name(name) => name.name.as_str(),
        Key::Text(text) => text.value.as_str(),
    }
}

/// Each key `value` gives twice in one object, at any depth, as written.
pub(crate) fn duplicate_keys(value: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    let mut walk = vec![value];
    while let Some(value) = walk.pop() {
        match value {
            Value::List(items, _) => walk.extend(items),
            Value::Object(fields, _) => {
                let mut seen = BTreeSet::new();
                for field in fields {
                    let key = key_text(&field.key);
                    if !seen.insert(key) {
                        problems.push(format!("`{key}` is given twice"));
                    }
                    walk.push(&field.value);
                }
            }
            _ => {}
        }
    }
    problems
}
