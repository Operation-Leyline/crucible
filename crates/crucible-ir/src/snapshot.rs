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
///
/// Format 2 dropped what only the removed protocols used (XML shapes, the
/// declared-protocol list, protocol tests, initial messages and the protocol
/// settings that were never set for Crucible JSON); [`read`] drops them from
/// a format 1 snapshot of a Crucible JSON service.
///
/// Format 3 dropped what only the retired Smithy front end could reach: streaming payloads
/// and event streams, waiters, endpoint rules, host prefixes and discovery,
/// HTTP checksums, non-bearer auth schemes, CORS, prefix headers, query-params
/// and response-code bindings, sets, and the JSON name
/// (a member's wire name is its name). [`read`] drops them from an older
/// snapshot that left them unused and refuses one that used them.
///
/// Format 4 dropped the OAuth `scopes` of the service and its operations.
/// [`read`] drops an empty list and refuses a snapshot that declared any.
///
/// Format 5 moved the built-in types and the standard vocabulary from the
/// `smithy.api` namespace to [`crate::PRELUDE`], `crucible.api` (decision
/// 62c in docs/idl/decisions.md), and dropped the service's `renames` and
/// each member's `client_optional`, which a `.cru` model never sets.
/// [`read`] maps every `smithy.api#..` id of an older snapshot to its
/// `crucible.api` id, drops `client_optional` and an empty `renames`, and
/// refuses a snapshot that renamed a shape. A `.cru` release never set
/// `client_optional`; one frozen from a Smithy model (`@input`,
/// `@clientOptional`) reads with the server's optionality, so a comparison
/// with it sees those members as the `.cru` model does. The file
/// itself is never rewritten: a release's lock digest is over its text as
/// written.
pub const FORMAT: u32 = 5;

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
/// more: no documentation strings (`docs`, example `docs`) and no raw trait maps, which repeat the typed IR (and
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
    service.doc_links.clear();
    service.doc_link_operations.clear();
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
        if let crate::ir::Requirements::List(clauses) | crate::ir::Requirements::Partial(clauses) =
            &mut op.requirements
        {
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
    let mut value = value;
    if format == 1 {
        migrate_format_1(&mut value)?;
    }
    if format <= 2 {
        migrate_format_2(&mut value)?;
    }
    drop_scopes(&mut value)?;
    if format <= 4 {
        migrate_format_4(&mut value)?;
    }
    value["format"] = serde_json::Value::from(FORMAT);
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

/// Reads a format 1 snapshot as format 2: removes the fields that only the
/// removed protocols used. A snapshot frozen with another protocol cannot be
/// read, since nothing implements that protocol any more.
fn migrate_format_1(value: &mut serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    let Some(service) = value.get_mut("service").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    if let Some(protocol) = service.get("protocol").and_then(Value::as_str)
        && protocol != "crucibleJson"
    {
        return Err(format!(
            "the release snapshot was frozen with protocol `{protocol}`, which Crucible no longer supports; only `crucibleJson` is supported"
        ));
    }
    for field in ["protocols", "xml_namespace", "xml_shapes", "protocol_tests"] {
        service.remove(field);
    }
    if let Some(settings) = service
        .get_mut("protocol_settings")
        .and_then(Value::as_object_mut)
    {
        for field in [
            "noErrorWrapping",
            "omitNullStructureMembers",
            "ignoreUnknownUnionType",
            "nonFiniteFloatStrings",
            "arbitraryPrecisionStrings",
        ] {
            settings.remove(field);
        }
    }
    for operation in service
        .get_mut("operations")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(operation) = operation.as_object_mut() {
            operation.remove("unwrapped_xml_output");
            for side in ["request", "response"] {
                if let Some(stream) = operation
                    .get_mut(side)
                    .and_then(|bindings| bindings.get_mut("event_stream"))
                    .and_then(Value::as_object_mut)
                {
                    stream.remove("initial_message");
                }
            }
        }
    }
    value["format"] = Value::from(2);
    Ok(())
}

/// What a format 3 snapshot no longer carries, by where it was. A field is
/// dropped when it holds its zero value and refuses the snapshot otherwise:
/// the snapshot used a feature Crucible no longer implements.
const REMOVED_SERVICE_FIELDS: &[&str] = &["cors", "endpoint_discovery", "endpoint_rules"];
const REMOVED_OPERATION_FIELDS: &[&str] = &[
    "unsigned_payload",
    "discovered_endpoint",
    "checksum",
    "optional_auth",
    "endpoint",
    "endpoint_parameters",
    "waiters",
];
const REMOVED_BINDING_FIELDS: &[&str] = &[
    "query_params",
    "prefix_headers",
    "response_code",
    "event_stream",
];
const REMOVED_ERROR_FIELDS: &[&str] = &["prefix_headers", "response_code"];
const REMOVED_PAYLOAD_FIELDS: &[&str] = &["streaming", "requires_length"];

/// Reads a snapshot of format 1 or 2 as format 3 (the scopes step makes it 4).
fn migrate_format_2(value: &mut serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    let Some(service) = value.get_mut("service").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    drop_unused(service, REMOVED_SERVICE_FIELDS, "the service")?;
    if let Some(settings) = service
        .get_mut("protocol_settings")
        .and_then(Value::as_object_mut)
    {
        settings.remove("eventStreamHttp");
    }
    if let Some(schemes) = service.get("auth_schemes").and_then(Value::as_object) {
        for (id, scheme) in schemes {
            if scheme.get("kind").and_then(Value::as_str) != Some("bearer") {
                return Err(unsupported(&format!("the auth scheme {id}")));
            }
        }
    }
    for operation in service
        .get_mut("operations")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object_mut)
    {
        let name = operation
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("an operation")
            .to_owned();
        drop_unused(operation, REMOVED_OPERATION_FIELDS, &name)?;
        for side in ["request", "response"] {
            if let Some(bindings) = operation.get_mut(side).and_then(Value::as_object_mut) {
                drop_unused(bindings, REMOVED_BINDING_FIELDS, &name)?;
                if let Some(payload) = bindings.get_mut("payload").and_then(Value::as_object_mut) {
                    drop_payload(payload, &name)?;
                }
            }
        }
    }
    for shape in service
        .get_mut("shapes")
        .and_then(Value::as_object_mut)
        .into_iter()
        .flat_map(|shapes| shapes.iter_mut())
    {
        let (id, shape) = shape;
        let Some((kind, body)) = shape
            .as_object_mut()
            .and_then(|shape| shape.iter_mut().next())
        else {
            continue;
        };
        let Some(body) = body.as_object_mut() else {
            continue;
        };
        match kind.as_str() {
            "Simple" => drop_unused(body, &["streaming", "requires_length"], id)?,
            "List" => drop_unused(body, &["is_set"], id)?,
            "Structure" | "Union" => {
                drop_unused(body, &["streaming"], id)?;
                if let Some(error) = body.get_mut("error").and_then(Value::as_object_mut) {
                    drop_unused(error, REMOVED_ERROR_FIELDS, id)?;
                    if let Some(payload) = error.get_mut("payload").and_then(Value::as_object_mut) {
                        drop_payload(payload, id)?;
                    }
                }
            }
            _ => {}
        }
        for member in members_mut(shape) {
            drop_unused(member, &["event_binding"], id)?;
            let name = member.get("name").cloned();
            match member.get("wire_name") {
                Some(wire) if Some(wire) != name.as_ref() => {
                    return Err(unsupported(&format!("a JSON name on a member of {id}")));
                }
                _ => {
                    member.remove("wire_name");
                }
            }
        }
    }
    value["format"] = Value::from(3);
    Ok(())
}

/// Drops the OAuth `scopes` every snapshot carried while Crucible modelled
/// them (an empty list for a service without any, which is every `.cru`
/// service) and refuses one that declared some.
fn drop_scopes(value: &mut serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    let Some(service) = value.get_mut("service").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    drop_scopes_of(service, "the service")?;
    for operation in service
        .get_mut("operations")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object_mut)
    {
        let name = operation
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("an operation")
            .to_owned();
        drop_scopes_of(operation, &name)?;
    }
    Ok(())
}

fn drop_scopes_of(
    object: &mut serde_json::Map<String, serde_json::Value>,
    owner: &str,
) -> Result<(), String> {
    if object.get("scopes").is_some_and(|value| !is_zero(value)) {
        return Err(format!(
            "the release snapshot declares OAuth scopes on {owner}, and Crucible no longer supports scopes"
        ));
    }
    object.remove("scopes");
    Ok(())
}

/// The namespace formats 1 to 4 spelled the built-in types and the standard
/// vocabulary in.
const OLD_PRELUDE: &str = "smithy.api";

/// Reads a format 4 (or older, already migrated) snapshot as format 5:
/// every id in the old prelude namespace becomes its [`crate::PRELUDE`]
/// id, wherever it appears (a member's target, an auth scheme, a map key),
/// and the fields a `.cru` model never set are dropped.
fn migrate_format_4(value: &mut serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    let Some(service) = value.get_mut("service") else {
        return Ok(());
    };
    map_prelude_ids(service)?;
    let Some(service) = service.as_object_mut() else {
        return Ok(());
    };
    if service.get("renames").is_some_and(|value| !is_zero(value)) {
        return Err(unsupported("shape renames"));
    }
    service.remove("renames");
    for shape in service
        .get_mut("shapes")
        .and_then(Value::as_object_mut)
        .into_iter()
        .flat_map(|shapes| shapes.values_mut())
    {
        // A client-optional member only relaxed what a client required,
        // which a `.cru` release never does: read as the server's
        // optionality, an older release compares with its `.cru` successor
        // as the guard compares two `.cru` releases.
        for member in members_mut(shape) {
            member.remove("client_optional");
        }
    }
    Ok(())
}

/// Fields of the serialized IR whose whole value is the model's data or
/// prose, never an id: defaults, examples, documentation, external doc
/// links.
const DATA_FIELDS: &[&str] = &[
    "default",
    "wire_default",
    "examples",
    "docs",
    "sunset_docs",
    "external_docs",
];

/// Fields whose string value is the model's text, never an id: an enum
/// value, a pattern, a deprecation message, an exception's reason, and a
/// member, header or query parameter name.
const TEXT_FIELDS: &[&str] = &["value", "pattern", "message", "reason", "name"];

/// Maps whose keys are labels and whose values are ids.
const LABEL_KEYED: &[&str] = &["doc_links", "doc_link_operations"];

/// Rewrites, in place, every id in the serialized IR `value` that `map`
/// renames: strings and map keys where the IR stores a shape, member,
/// trait or auth scheme id, and not the model's own data ([`DATA_FIELDS`],
/// [`TEXT_FIELDS`], raw trait values, doc link labels). A map that would
/// hold one key twice afterwards is refused, with that key, rather than
/// merged.
pub(crate) fn rewrite_ids(
    value: &mut serde_json::Value,
    map: &dyn Fn(&str) -> Option<String>,
) -> Result<(), String> {
    use serde_json::Value;
    match value {
        Value::String(text) => {
            if let Some(new) = map(text) {
                *text = new;
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite_ids(item, map)?;
            }
        }
        Value::Object(fields) => {
            for (key, mut field) in std::mem::take(fields) {
                let data = DATA_FIELDS.contains(&key.as_str())
                    || (TEXT_FIELDS.contains(&key.as_str()) && field.is_string());
                if key == "traits" {
                    // Keys are trait ids; values are the author's.
                    if let Value::Object(traits) = &mut field {
                        for (id, application) in std::mem::take(traits) {
                            insert(traits, map(&id).unwrap_or(id), application)?;
                        }
                    }
                } else if LABEL_KEYED.contains(&key.as_str()) {
                    if let Value::Object(links) = &mut field {
                        for target in links.values_mut() {
                            rewrite_ids(target, map)?;
                        }
                    }
                } else if !data {
                    rewrite_ids(&mut field, map)?;
                }
                insert(fields, map(&key).unwrap_or(key), field)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn insert(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    if fields.contains_key(&key) {
        return Err(key);
    }
    fields.insert(key, value);
    Ok(())
}

/// Rewrites every id in the old prelude namespace to its [`crate::PRELUDE`]
/// id ([`rewrite_ids`]). Prose that happens to mention one is not an id
/// (it does not parse as one) and is left alone.
fn map_prelude_ids(value: &mut serde_json::Value) -> Result<(), String> {
    let mapped = |text: &str| {
        let rest = text.strip_prefix(OLD_PRELUDE)?.strip_prefix('#')?;
        text.parse::<crate::ShapeId>()
            .ok()
            .map(|_| format!("{}#{rest}", crate::PRELUDE))
    };
    rewrite_ids(value, &mapped).map_err(|key| {
        format!(
            "the release snapshot names `{key}` in both the `{OLD_PRELUDE}` and the `{}` spelling",
            crate::PRELUDE
        )
    })
}

/// The members of a serialized shape: a structure's or union's, a list's
/// one, a map's key and value.
fn members_mut(
    shape: &mut serde_json::Value,
) -> Vec<&mut serde_json::Map<String, serde_json::Value>> {
    use serde_json::Value;
    let Some((kind, body)) = shape
        .as_object_mut()
        .and_then(|shape| shape.iter_mut().next())
    else {
        return Vec::new();
    };
    let Some(body) = body.as_object_mut() else {
        return Vec::new();
    };
    let members: Vec<&mut Value> = match kind.as_str() {
        "Structure" | "Union" => body
            .get_mut("members")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
            .collect(),
        "List" => body.get_mut("member").into_iter().collect(),
        "Map" => body
            .iter_mut()
            .filter(|(field, _)| *field == "key" || *field == "value")
            .map(|(_, member)| member)
            .collect(),
        _ => Vec::new(),
    };
    members
        .into_iter()
        .filter_map(Value::as_object_mut)
        .collect()
}

/// Removes the `fields` of `object` that hold their zero value; one that
/// holds anything else was used.
fn drop_unused(
    object: &mut serde_json::Map<String, serde_json::Value>,
    fields: &[&str],
    owner: &str,
) -> Result<(), String> {
    for field in fields {
        if object.get(*field).is_some_and(|value| !is_zero(value)) {
            return Err(unsupported(&format!("`{field}` on {owner}")));
        }
        object.remove(*field);
    }
    Ok(())
}

fn drop_payload(
    payload: &mut serde_json::Map<String, serde_json::Value>,
    owner: &str,
) -> Result<(), String> {
    if payload.get("kind").and_then(serde_json::Value::as_str) == Some("eventStream") {
        return Err(unsupported(&format!("an event stream on {owner}")));
    }
    drop_unused(payload, REMOVED_PAYLOAD_FIELDS, owner)
}

fn unsupported(what: &str) -> String {
    format!(
        "the release snapshot uses {what}, which Crucible no longer supports (see the Migration entries in the changelog)"
    )
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// Ids are mapped where the IR stores them; the model's own data that
    /// happens to spell one is left as written.
    #[test]
    fn only_id_positions_are_mapped() {
        let old = "smithy.api#String";
        let mut service = json!({
            "auth": ["smithy.api#httpBearerAuth"],
            "auth_schemes": {"smithy.api#httpBearerAuth": {"kind": "bearer"}},
            "internal_traits": ["smithy.api#internalNote"],
            "doc_links": {"smithy.api#String": "smithy.api#String"},
            "doc_link_operations": {"Op.member": "example#Op"},
            "exceptions": [{"rule": "r", "reason": old}],
            "shapes": {
                "example#Name": {"Simple": {
                    "kind": "String",
                    "constraints": {"pattern": old},
                    "notes": {"deprecated": {"message": old, "since": null}},
                    "docs": old,
                    "traits": {"smithy.api#documentation": old, "example#tag": old}
                }},
                "example#Kind": {"Enum": {"values": [{"name": "A", "value": old}]}},
                "example#Shape": {"Structure": {"members": [{
                    "name": "m",
                    "target": old,
                    "default": old,
                    "wire_default": old
                }]}}
            },
            "operations": [{
                "id": "example#Op",
                "examples": [{"input": {"m": old}}],
                "request": {"headers": [{"member": "m", "name": old}]}
            }]
        });
        super::map_prelude_ids(&mut service).unwrap();
        // Mapped: ids, as values and as keys.
        assert_eq!(service["auth"][0], "crucible.api#httpBearerAuth");
        assert!(service["auth_schemes"]["crucible.api#httpBearerAuth"].is_object());
        assert_eq!(service["internal_traits"][0], "crucible.api#internalNote");
        assert!(
            service["shapes"]["example#Name"]["Simple"]["traits"]["crucible.api#documentation"]
                .is_string()
        );
        let member = &service["shapes"]["example#Shape"]["Structure"]["members"][0];
        assert_eq!(member["target"], "crucible.api#String");
        // A doc link's label is text; its target is an id.
        assert_eq!(
            service["doc_links"]["smithy.api#String"],
            "crucible.api#String"
        );
        // Left alone: defaults, examples, docs, patterns, messages, reasons,
        // enum values, names and raw trait values.
        assert_eq!(member["default"], old);
        assert_eq!(member["wire_default"], old);
        assert_eq!(service["operations"][0]["examples"][0]["input"]["m"], old);
        let simple = &service["shapes"]["example#Name"]["Simple"];
        assert_eq!(simple["docs"], old);
        assert_eq!(simple["constraints"]["pattern"], old);
        assert_eq!(simple["notes"]["deprecated"]["message"], old);
        assert_eq!(simple["traits"]["crucible.api#documentation"], old);
        assert_eq!(simple["traits"]["example#tag"], old);
        assert_eq!(
            service["shapes"]["example#Kind"]["Enum"]["values"][0]["value"],
            old
        );
        assert_eq!(service["exceptions"][0]["reason"], old);
        assert_eq!(
            service["operations"][0]["request"]["headers"][0]["name"],
            old
        );
    }
}
