//! A frozen release's model (docs/api-policy.md, "Releases"): the lowered
//! IR as canonical JSON, read straight back into the IR. Older releases are
//! never recompiled from source, so a compiler or language change cannot
//! change what a released contract means.

use serde::{Deserialize, Serialize};

use crate::ir::Service;

/// The snapshot format this compiler writes. A reader accepts every format
/// up to it; a change that old snapshots cannot be read with bumps it and
/// teaches `read` the older one. A field added with a default needs no bump:
/// an older compiler refuses a snapshot with a field it does not know
/// rather than drop it.
pub const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Snapshot {
    /// Which format the rest of the file is in.
    format: u32,
    /// The compiler that froze it, for the record.
    compiler: String,
    service: Service,
}

/// `service` as a release snapshot: pretty-printed and deterministic, so a
/// release's file diffs as the API changed. Older versions the service
/// serves are not part of a release.
pub fn write(service: &Service, compiler: &str) -> String {
    let mut service = service.clone();
    service.versions.clear();
    let snapshot = Snapshot {
        format: FORMAT,
        compiler: compiler.to_owned(),
        service,
    };
    // Through the reader once, so each number is spelled as reading it back
    // spells it (`4.7e-6` reads as `0.0000047`): writing what was read
    // gives the same file.
    let first = serde_json::to_value(&snapshot).expect("the IR serializes");
    let snapshot: Snapshot = serde_json::from_value(first).expect("the IR reads back");
    let mut text = serde_json::to_string_pretty(&snapshot).expect("the IR serializes");
    text.push('\n');
    text
}

/// The model a snapshot froze.
pub fn read(text: &str) -> Result<Service, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("not a release snapshot: {error}"))?;
    let format = value
        .get("format")
        .and_then(serde_json::Value::as_u64)
        .ok_or("not a release snapshot: no `format`")?;
    if format == 0 {
        return Err("not a release snapshot: format 0".into());
    }
    if format > u64::from(FORMAT) {
        return Err(format!(
            "release snapshot format {format} is newer than this compiler reads ({FORMAT}); update Crucible"
        ));
    }
    let snapshot: Snapshot = serde_json::from_value(value.clone())
        .map_err(|error| format!("release snapshot format {format}: {error}"))?;
    // Whatever this compiler does not know would be dropped silently: a
    // newer compiler wrote it, and only a newer one reads it right.
    let read = serde_json::to_value(&snapshot).expect("the IR serializes");
    if let Some(path) = unknown(&value, &read, String::new()) {
        return Err(format!(
            "the release snapshot has `{path}`, which this compiler does not know; a newer Crucible wrote it: update Crucible"
        ));
    }
    Ok(snapshot.service)
}

/// The first field `original` has that `read`, the same snapshot read and
/// written back, does not.
fn unknown(original: &serde_json::Value, read: &serde_json::Value, at: String) -> Option<String> {
    use serde_json::Value;
    match (original, read) {
        (Value::Object(original), Value::Object(read)) => {
            original.iter().find_map(|(key, value)| {
                let path = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                match read.get(key) {
                    Some(known) => unknown(value, known, path),
                    // A field at its zero value, which this compiler may skip
                    // writing: reading it as absent changes nothing.
                    None if is_zero(value) => None,
                    None => Some(path),
                }
            })
        }
        (Value::Array(original), Value::Array(read)) => {
            if original.len() != read.len() {
                return Some(format!("{at}[{}]", read.len().min(original.len())));
            }
            original
                .iter()
                .zip(read)
                .enumerate()
                .find_map(|(index, (value, known))| unknown(value, known, format!("{at}[{index}]")))
        }
        _ => None,
    }
}

fn is_zero(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => true,
        serde_json::Value::Bool(value) => !value,
        serde_json::Value::Number(number) => number.as_f64() == Some(0.0),
        serde_json::Value::String(text) => text.is_empty(),
        serde_json::Value::Array(items) => items.is_empty(),
        serde_json::Value::Object(fields) => fields.is_empty(),
    }
}
