use std::collections::BTreeMap;

use crucible_smithy::ir::{Member, Service, Shape, SimpleKind, TimestampFormat};
use crucible_smithy::{Diagnostics, ShapeId};
use serde_json::{Value, json};

use crate::{Options, names};

const RESERVED_FLAGS: &[&str] = &[
    "help",
    "endpoint-url",
    "profile",
    "config",
    "output",
    "generate-completion",
    "input-json",
    "generate-input",
    "no-paginate",
];

pub(crate) fn emit(service: &Service, options: &Options, diagnostics: &mut Diagnostics) -> String {
    let mut commands = BTreeMap::new();
    let mut operations = Vec::new();
    let mut sorted = service.operations.iter().collect::<Vec<_>>();
    sorted.sort_by_key(|operation| operation.name());
    for operation in sorted {
        let command = names::kebab(operation.name());
        if command.is_empty() || command == "help" {
            diagnostics.push(
                &operation.id,
                format!("CLI command `{command}` is reserved"),
            );
        }
        if let Some(previous) = commands.insert(command.clone(), &operation.id) {
            diagnostics.push(
                &operation.id,
                format!("CLI command `{command}` collides with {previous}"),
            );
        }
        if let Some((id, Shape::Structure(structure))) = operation
            .input
            .as_ref()
            .and_then(|id| service.shape(id).map(|shape| (id, shape)))
        {
            let mut flags = BTreeMap::new();
            for member in &structure.members {
                let flag = names::kebab(&member.name);
                if flag.is_empty() || RESERVED_FLAGS.contains(&flag.as_str()) {
                    diagnostics.push(
                        &id.with_member(&member.name),
                        format!("CLI flag `--{flag}` is reserved"),
                    );
                }
                if let Some(previous) = flags.insert(flag.clone(), &member.name) {
                    diagnostics.push(
                        &id.with_member(&member.name),
                        format!("CLI flag `--{flag}` collides with member `{previous}`"),
                    );
                }
            }
        }
        operations.push(json!({
            "name": command,
            "operation": operation.name(),
            "description": operation.docs.as_deref().unwrap_or(""),
            "input": operation.input.as_ref().map(ToString::to_string),
            "paginated": operation.paginated.is_some(),
        }));
    }
    let shapes: BTreeMap<String, Value> = service
        .shapes
        .iter()
        .map(|(id, shape)| (id.to_string(), describe(service, shape)))
        .collect();
    let schema = json!({
        "name": options.bin_name,
        "version": options.package_version,
        "description": service.docs.as_deref().or(service.title.as_deref()).unwrap_or(service.id.name()),
        "env_prefix": options.bin_name.replace('-', "_").to_ascii_uppercase(),
        "endpoint": options.endpoint,
        "operations": operations,
        "shapes": shapes,
    });
    serde_json::to_string_pretty(&schema).expect("JSON serializes") + "\n"
}

fn describe(service: &Service, shape: &Shape) -> Value {
    let mut value = match shape {
        Shape::Unit => json!({"kind": "unit"}),
        Shape::Simple(simple) => json!({"kind": match simple.kind {
            SimpleKind::Boolean => "boolean",
            SimpleKind::String => "string",
            SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long => "integer",
            SimpleKind::Float | SimpleKind::Double => "number",
            SimpleKind::Timestamp => "timestamp",
            SimpleKind::Document => "document",
            SimpleKind::Blob => "blob",
            // Lowering diagnoses these: the CLI generator declares no big-number capability.
            SimpleKind::BigInteger | SimpleKind::BigDecimal => {
                unreachable!("big numbers require a target capability")
            }
        }}),
        // Like the SDK, accept values introduced by a newer service model.
        Shape::Enum(_) => json!({"kind": "string"}),
        Shape::IntEnum(_) => json!({"kind": "integer"}),
        Shape::List(list) => json!({"kind": "list", "target": list.member.target.to_string()}),
        Shape::Map(map) => json!({"kind": "map", "target": map.value.target.to_string()}),
        Shape::Structure(structure) | Shape::Union(structure) => json!({
            "kind": if matches!(shape, Shape::Union(_)) { "union" } else { "structure" },
            "members": structure.members.iter().map(|member| describe_member(service, member)).collect::<Vec<_>>()
        }),
    };
    value["sensitive"] = json!(shape.sensitive());
    value
}

fn describe_member(service: &Service, member: &Member) -> Value {
    let mut value = json!({
        "name": member.name,
        // The SDK's serde keys the member by its wire name, not its model name.
        "wire_name": member.wire_name,
        "flag": names::kebab(&member.name),
        "target": member.target.to_string(),
        "description": member.docs.as_deref().unwrap_or(""),
        "required": !member.optional_on_client() && member.client_default().is_none()
            && !member.traits.contains_key(&ShapeId::new("smithy.api", "idempotencyToken")),
        "sensitive": service.sensitive(member),
    });
    if matches!(service.shape(&member.target), Some(Shape::Simple(simple)) if simple.kind == SimpleKind::Timestamp)
    {
        // The SDK field is parsed by the document codec; HTTP bindings only
        // change how the value is spelled on the command line.
        let document = service.document_timestamp_format(member);
        value["kind"] = json!(match document {
            TimestampFormat::EpochSeconds => "number",
            TimestampFormat::DateTime | TimestampFormat::HttpDate => "string",
        });
        value["timestamp_format"] = json!(document);
        let bound = service.timestamp_format(member);
        if bound != document {
            value["bound_format"] = json!(bound);
        }
    }
    value
}
