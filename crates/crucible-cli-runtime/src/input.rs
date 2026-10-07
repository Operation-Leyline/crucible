use std::collections::BTreeSet;
use std::io::Read;

use clap::ArgMatches;
use serde_json::{Map, Value};

use crate::Error;
use crate::spec::{Kind, Operation, Spec};

pub(crate) fn request(
    spec: &Spec,
    operation: &Operation,
    matches: &ArgMatches,
) -> Result<Value, Error> {
    let mut value = match matches.get_one::<String>("input-json") {
        Some(source) => read_json(source)?,
        None => Value::Object(Map::new()),
    };
    let object = value
        .as_object_mut()
        .ok_or_else(|| Error::input("request input must be a JSON object"))?;
    for member in spec.members(operation)? {
        if let Some(raw) = matches.get_one::<String>(&member.flag) {
            let kind = member.kind.unwrap_or(spec.shape(&member.target)?.kind);
            let parsed = argument(kind, raw).map_err(|_| {
                Error::input(format!(
                    "invalid value for --{}{}",
                    member.flag,
                    if member.sensitive {
                        " (sensitive value omitted)"
                    } else {
                        ""
                    }
                ))
            })?;
            object.insert(member.name.clone(), parsed);
        }
    }
    match &operation.input {
        Some(target) => validate(spec, target, &value, "input", false)?,
        None if !object.is_empty() => return Err(Error::input("this operation takes no input")),
        None => {}
    }
    Ok(value)
}

fn read_json(source: &str) -> Result<Value, Error> {
    let text = if source == "-" {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(Error::input)?;
        text
    } else if let Some(path) = source.strip_prefix("file://") {
        std::fs::read_to_string(path)
            .map_err(|error| Error::input(format!("cannot read input file: {error}")))?
    } else {
        source.to_owned()
    };
    serde_json::from_str(&text).map_err(|error| {
        Error::input(format!(
            "invalid request JSON at line {}, column {}",
            error.line(),
            error.column()
        ))
    })
}

fn argument(kind: Kind, raw: &str) -> Result<Value, Error> {
    match kind {
        Kind::String | Kind::Timestamp => Ok(Value::String(raw.to_owned())),
        Kind::Blob => {
            let blob = if let Some(path) = raw.strip_prefix("fileb://") {
                crucible_client_runtime_rust::Blob::new(std::fs::read(path).map_err(Error::input)?)
            } else {
                crucible_client_runtime_rust::Blob::from_base64(raw)
                    .ok_or_else(|| Error::input("expected base64 or fileb://path"))?
            };
            Ok(Value::String(blob.to_base64()))
        }
        _ => {
            let value: Value = serde_json::from_str(raw).map_err(Error::input)?;
            let valid = match kind {
                Kind::Boolean => value.is_boolean(),
                Kind::Integer => value.is_i64() || value.is_u64(),
                Kind::Number => value.is_number(),
                Kind::Structure | Kind::Union | Kind::Map | Kind::Unit => value.is_object(),
                Kind::List => value.is_array(),
                Kind::Document => true,
                _ => unreachable!(),
            };
            if valid {
                Ok(value)
            } else {
                Err(Error::input("incorrect argument type"))
            }
        }
    }
}

// SDK deserialization checks scalar types. This pass additionally catches
// unknown object keys that serde deliberately ignores in forward-compatible SDKs.
fn validate(
    spec: &Spec,
    target: &str,
    value: &Value,
    path: &str,
    sensitive: bool,
) -> Result<(), Error> {
    if value.is_null() {
        return Ok(());
    }
    let shape = spec.shape(target)?;
    let sensitive = sensitive || shape.sensitive;
    match shape.kind {
        Kind::Structure | Kind::Union | Kind::Unit => {
            let object = value
                .as_object()
                .ok_or_else(|| Error::input(format!("{path} must be an object")))?;
            if shape.kind == Kind::Union && object.len() != 1 {
                return Err(Error::input(format!(
                    "{path} must contain exactly one union member"
                )));
            }
            for (name, value) in object {
                let member = shape
                    .members
                    .iter()
                    .find(|member| member.name == *name)
                    .ok_or_else(|| {
                        if sensitive {
                            Error::input(format!(
                                "unknown member in {path} (sensitive name omitted)"
                            ))
                        } else {
                            Error::input(format!("unknown member {path}.{name}"))
                        }
                    })?;
                validate(
                    spec,
                    &member.target,
                    value,
                    &format!("{path}.{name}"),
                    sensitive || member.sensitive,
                )?;
            }
            for member in &shape.members {
                if member.required && !object.contains_key(&member.name) {
                    return Err(Error::input(format!(
                        "missing required member {path}.{}",
                        member.name
                    )));
                }
            }
        }
        Kind::List => {
            if let (Some(values), Some(target)) = (value.as_array(), &shape.target) {
                for (index, value) in values.iter().enumerate() {
                    validate(spec, target, value, &format!("{path}[{index}]"), sensitive)?;
                }
            }
        }
        Kind::Map => {
            if let (Some(values), Some(target)) = (value.as_object(), &shape.target) {
                // Map keys are user values, not modeled member names, and may
                // themselves be credentials even on an unannotated map.
                for value in values.values() {
                    validate(spec, target, value, &format!("{path}[value]"), sensitive)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn skeleton(spec: &Spec, operation: &Operation) -> Result<Value, Error> {
    match &operation.input {
        Some(target) => sample(spec, target, &mut BTreeSet::new()),
        None => Ok(Value::Object(Map::new())),
    }
}

fn sample(spec: &Spec, target: &str, seen: &mut BTreeSet<String>) -> Result<Value, Error> {
    if !seen.insert(target.to_owned()) {
        return Ok(Value::Null);
    }
    let shape = spec.shape(target)?;
    let result = match shape.kind {
        Kind::Structure | Kind::Union | Kind::Unit => {
            let mut object = Map::new();
            for member in &shape.members {
                let value = match member.kind {
                    Some(Kind::Number | Kind::Integer) => Value::from(0),
                    Some(Kind::String) => Value::String(String::new()),
                    _ => sample(spec, &member.target, seen)?,
                };
                object.insert(member.name.clone(), value);
                if shape.kind == Kind::Union {
                    break;
                }
            }
            Value::Object(object)
        }
        Kind::Boolean => Value::Bool(false),
        Kind::Integer | Kind::Number => Value::from(0),
        Kind::Timestamp => Value::String("1970-01-01T00:00:00Z".to_owned()),
        Kind::String | Kind::Blob => Value::String(String::new()),
        Kind::List => Value::Array(Vec::new()),
        Kind::Map => Value::Object(Map::new()),
        Kind::Document => Value::Null,
    };
    seen.remove(target);
    Ok(result)
}
