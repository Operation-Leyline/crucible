//! `errors.ts`: one `ServiceError` subclass per modelled error, so callers
//! branch with `instanceof` and read typed fields.

use crucible_smithy::Diagnostics;
use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Fault, Service, Shape, Structure};

use crate::names::{doc, property, ts_type};

/// Properties an error inherits, which a member must not shadow.
const INHERITED: &[&str] = &[
    "name",
    "stack",
    "cause",
    "$metadata",
    "$fault",
    "$retryable",
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
    let mut out = String::new();
    for (id, structure) in error_shapes(service) {
        let name = service.name_of(id);
        let error = structure.error.as_ref().expect("filtered to errors");
        out.push('\n');
        doc(&mut out, "", structure.docs.as_deref());
        out.push_str(&format!("export class {name} extends ServiceError {{\n"));
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
                continue;
            }
            doc(&mut out, "  ", member.docs.as_deref());
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
        out.push_str(&format!(
            "  constructor(fields: globalThis.Record<string, unknown>, metadata: ResponseMetadata) {{\n    \
             super({}, typeof fields.message === \"string\" ? fields.message : \"\", metadata, \"{fault}\", {retryable});\n\
             {assignments}  }}\n}}\n",
            crate::names::literal(&error.wire_code)
        ));
    }
    let mut header = String::from(crate::HEADER);
    header.push_str("import { ServiceError } from \"./runtime.js\";\n");
    header.push_str(&crate::names::runtime_types(&out, &["ResponseMetadata"]));
    if out.contains("m.") {
        header.push_str("import type * as m from \"./models.js\";\n");
    }
    header + &out
}
