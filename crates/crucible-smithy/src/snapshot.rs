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

/// `service` as a package carries it (`crucible-model.json`): the release
/// snapshot of the typed contract, as compact JSON. A package ships in
/// every release, so it holds what a compatibility comparison reads and no
/// more: no documentation strings (`docs`, example `docs`, waiter
/// `documentation`) and no raw trait maps, which repeat the typed IR (and
/// carry documentation, raw vectors and legacy enum definitions with it).
/// [`read`] reads it like any snapshot.
pub fn write_contract(service: &Service, compiler: &str) -> String {
    let mut service = service.clone();
    typed_contract(&mut service);
    let text = write(&service, compiler);
    let value: serde_json::Value = serde_json::from_str(&text).expect("a snapshot is JSON");
    let mut text = serde_json::to_string(&value).expect("JSON serializes");
    text.push('\n');
    text
}

/// Clears every documentation string and raw trait map from `service`.
fn typed_contract(service: &mut Service) {
    use crate::ir::{Member, Shape, Traits};
    fn strip(traits: &mut Traits) {
        traits.clear();
    }
    fn member(member: &mut Member) {
        member.docs = None;
        strip(&mut member.traits);
    }
    service.docs = None;
    service.internal_traits.clear();
    strip(&mut service.traits);
    for resource in service.resources.values_mut() {
        resource.docs = None;
        strip(&mut resource.traits);
    }
    for op in &mut service.operations {
        op.docs = None;
        strip(&mut op.traits);
        for example in &mut op.examples {
            example.docs = None;
        }
        for waiter in op.waiters.values_mut() {
            waiter.documentation = None;
        }
        if let crate::ir::Requirements::List(clauses) = &mut op.requirements {
            for clause in clauses {
                clause.docs = None;
            }
        }
    }
    for resource in service.permissions.values_mut() {
        for permission in &mut resource.permissions {
            permission.docs = None;
        }
    }
    for shape in service.shapes.values_mut() {
        match shape {
            Shape::Unit { .. } => {}
            Shape::Simple(s) => {
                s.docs = None;
                strip(&mut s.traits);
            }
            Shape::Enum(s) => {
                s.docs = None;
                strip(&mut s.traits);
                for value in &mut s.values {
                    value.docs = None;
                    strip(&mut value.traits);
                }
            }
            Shape::IntEnum(s) => {
                s.docs = None;
                strip(&mut s.traits);
                for value in &mut s.values {
                    value.docs = None;
                    strip(&mut value.traits);
                }
            }
            Shape::List(s) => {
                s.docs = None;
                strip(&mut s.traits);
                member(&mut s.member);
            }
            Shape::Map(s) => {
                s.docs = None;
                strip(&mut s.traits);
                member(&mut s.key);
                member(&mut s.value);
            }
            Shape::Structure(s) | Shape::Union(s) => {
                s.docs = None;
                strip(&mut s.traits);
                s.members.iter_mut().for_each(member);
            }
        }
    }
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
