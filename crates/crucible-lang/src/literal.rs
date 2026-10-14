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
/// differ when `SPECIAL = "RED"`. Map keys are text, never symbols.
pub(crate) fn resolve_enum_symbols(
    shapes: &BTreeMap<ShapeId, Shape>,
    target: &ShapeId,
    value: &Value,
    json: &mut Json,
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
                resolve_enum_symbols(shapes, &list.member.target, item, json);
            }
        }
        (
            Shape::Map(_) | Shape::Structure(_) | Shape::Union(_),
            Value::Object(fields, _),
            Json::Object(object),
        ) => {
            // A key given twice keeps its last value, as the conversion does.
            let mut seen = BTreeSet::new();
            for field in fields.iter().rev() {
                let key = match &field.key {
                    Key::Name(name) => name.name.as_str(),
                    Key::Text(text) => text.value.as_str(),
                };
                if !seen.insert(key) {
                    continue;
                }
                let target = match shape {
                    Shape::Map(map) => Some(&map.value.target),
                    Shape::Structure(s) | Shape::Union(s) => s.member(key).map(|m| &m.target),
                    _ => None,
                };
                if let (Some(target), Some(json)) = (target, object.get_mut(key)) {
                    resolve_enum_symbols(shapes, target, &field.value, json);
                }
            }
        }
        _ => {}
    }
}
