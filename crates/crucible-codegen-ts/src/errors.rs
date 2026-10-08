//! `errors.ts`: one `ServiceError` subclass per modelled error, so callers
//! branch with `instanceof` and read typed fields.

use crucible_smithy::Diagnostics;
use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Fault, Service, Shape, Structure};

use crate::names::{doc, documented, property, ts_type};

/// Properties an error inherits, which a member must not shadow.
const INHERITED: &[&str] = &[
    "name",
    "stack",
    "cause",
    "$metadata",
    "$fault",
    "$retryable",
    "toString",
];

pub(crate) fn error_shapes(service: &Service) -> Vec<(&ShapeId, &Structure)> {
    let mut errors: Vec<(&ShapeId, &Structure)> = service
        .shapes
        .iter()
        .filter_map(|(id, shape)| match shape {
            Shape::Structure(structure) if structure.error.is_some() => Some((id, structure)),
            _ => None,
        })
        .collect();
    errors.sort_by_key(|(id, _)| service.name_of(id));
    errors
}

pub(crate) fn emit(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let exception = crucible_codegen_common::service_exception_name(service);
    let mut out = String::new();
    for (id, structure) in error_shapes(service) {
        let name = service.name_of(id);
        let error = structure.error.as_ref().expect("filtered to errors");
        out.push('\n');
        documented(&mut out, "", structure.docs.as_deref(), &structure.notes);
        out.push_str(&format!("export class {name} extends {exception} {{\n"));
        for member in &structure.members {
            if INHERITED.contains(&member.name.as_str()) {
                diagnostics.push(
                    &id.with_member(&member.name),
                    format!(
                        "an error member cannot be named `{}` in TypeScript",
                        member.name
                    ),
                );
                continue;
            }
            if member.name == "message" {
                documented(&mut out, "  ", member.docs.as_deref(), &member.notes);
                out.push_str("  declare message: string;\n");
                continue;
            }
            documented(&mut out, "  ", member.docs.as_deref(), &member.notes);
            let marker = if member.optional_on_client() { "?" } else { "" };
            out.push_str(&format!(
                "  declare readonly {}{marker}: {};\n",
                property(&member.name),
                ts_type(service, &member.target, "m.")
            ));
        }
        let fault = match error.fault {
            Fault::Client => "client",
            Fault::Server => "server",
        };
        let retryable = error.retryable.map_or("undefined".to_owned(), |r| {
            format!("{{ throttling: {} }}", r.throttling)
        });
        if !out.ends_with("{\n") {
            out.push('\n');
        }
        let assignments = structure.members.iter().filter(|member| member.name != "message").map(|member| {
            let name = crate::names::literal(&member.name);
            format!("    if (globalThis.Object.hasOwn(fields, {name})) globalThis.Object.defineProperty(this, {name}, {{ value: fields[{name}], enumerable: true }});\n")
        }).collect::<Vec<_>>().join("");
        doc(
            &mut out,
            "  ",
            Some("Construct from decoded model fields and HTTP response metadata."),
        );
        out.push_str(&format!(
            "  constructor(fields: globalThis.Record<string, unknown>, metadata: ResponseMetadata) {{\n    \
             super({}, typeof fields.message === \"string\" ? fields.message : \"\", metadata, \"{fault}\", {retryable});\n\
             {assignments}  }}\n}}\n",
            crate::names::literal(&error.wire_code)
        ));
        // Models are interfaces; only errors have a class representation.
        let end = out.len() - 2;
        out.insert_str(end, &format!("  /** Describe modeled members with recursive sensitive-data redaction. */\n  override toString(): string {{ return formatModel(debugShapes, {}, this); }}\n  /** Use the same redaction when inspected by Node.js. */\n  [globalThis.Symbol.for(\"nodejs.util.inspect.custom\")](): string {{ return this.toString(); }}\n", crate::names::literal(&id.to_string())));
    }
    let mut header = String::from(crate::HEADER);
    header.push_str(&format!("import {{ {exception}, formatModel }} from \"./runtime.js\";\nexport {{ {exception} }} from \"./runtime.js\";\n"));
    header.push_str(&crate::names::runtime_types(&out, &["ResponseMetadata"]));
    if out.contains("m.") {
        header.push_str("import type * as m from \"./models.js\";\n");
    }
    let mut shapes = crucible_codegen_common::schema(service, str::to_owned)["shapes"].clone();
    // Only error classes need formatting metadata, including their nested targets.
    let mut pending: Vec<_> = error_shapes(service)
        .iter()
        .map(|(id, _)| id.to_string())
        .collect();
    let mut reachable = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !reachable.insert(id.clone()) {
            continue;
        }
        let shape = &shapes[&id];
        let mut members: Vec<_> = shape["members"]
            .as_object()
            .map_or_else(Vec::new, |members| members.values().collect());
        members.extend(
            ["member", "key", "value"]
                .iter()
                .filter_map(|key| shape.get(key)),
        );
        pending.extend(
            members
                .iter()
                .filter_map(|member| member["target"].as_str().map(str::to_owned)),
        );
    }
    shapes
        .as_object_mut()
        .expect("shape table")
        .retain(|id, _| reachable.contains(id));
    for (id, shape) in &service.shapes {
        if shape.sensitive() && reachable.contains(&id.to_string()) {
            shapes[id.to_string()]["sensitive"] = serde_json::json!(true);
        }
    }
    // Keep only formatting metadata; transport/error metadata is unrelated.
    for shape in shapes.as_object_mut().expect("shape table").values_mut() {
        shape.as_object_mut().expect("shape").retain(|key, _| {
            [
                "kind",
                "name",
                "sensitive",
                "streaming",
                "members",
                "member",
                "key",
                "value",
            ]
            .contains(&key.as_str())
        });
        for (key, value) in shape.as_object_mut().expect("shape").iter_mut() {
            if key == "members" {
                for member in value.as_object_mut().expect("members").values_mut() {
                    debug_member(member);
                }
            } else if ["member", "key", "value"].contains(&key.as_str()) {
                debug_member(value);
            }
        }
    }
    header
        + &format!(
            "\nconst debugShapes = globalThis.JSON.parse({});\n",
            crate::names::literal(&shapes.to_string())
        )
        + &out
}

fn debug_member(member: &mut serde_json::Value) {
    member
        .as_object_mut()
        .expect("member")
        .retain(|key, _| ["target", "attribute", "sensitive"].contains(&key.as_str()));
}
