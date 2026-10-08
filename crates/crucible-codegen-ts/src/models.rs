//! `models.ts`: a type for every shape the service reaches. Error shapes are
//! classes in `errors.ts` instead.

use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Service, Shape, SimpleKind, Structure};

use crate::names::{doc, literal, optional, property, ts_type};

pub(crate) fn emit(service: &Service) -> String {
    let mut out = String::new();
    for (id, shape) in model_shapes(service) {
        out.push('\n');
        let name = service.name_of(id);
        match shape {
            Shape::IntEnum(values) => {
                doc(&mut out, "", values.docs.as_deref());
                out.push_str(&format!("export const {name} = {{\n"));
                for value in &values.values {
                    doc(&mut out, "  ", value.docs.as_deref());
                    out.push_str(&format!("  {}: {},\n", property(&value.name), value.value));
                }
                out.push_str("} as const;\n");
                doc(
                    &mut out,
                    "",
                    values.docs.as_deref().or(Some(
                        "Open integer values, including unrecognized server values.",
                    )),
                );
                out.push_str(&format!(
                    "export type {name} = (typeof {name})[keyof typeof {name}] | (number & {{}});\n"
                ));
            }
            Shape::Unit { .. } => {}
            Shape::Simple(simple) => {
                doc(&mut out, "", simple.docs.as_deref());
                let ts = match simple.kind {
                    SimpleKind::Boolean => "boolean",
                    SimpleKind::String => "string",
                    SimpleKind::Long | SimpleKind::BigInteger => "bigint",
                    SimpleKind::BigDecimal => "Decimal",
                    SimpleKind::Timestamp => "globalThis.Date",
                    SimpleKind::Document => "Document",
                    SimpleKind::Blob if simple.streaming => "ByteStream",
                    SimpleKind::Blob => "globalThis.Uint8Array",
                    _ => "number",
                };
                out.push_str(&format!("export type {name} = {ts};\n"));
            }
            Shape::Enum(values) => {
                doc(&mut out, "", values.docs.as_deref());
                out.push_str(&format!("export const {name} = {{\n"));
                for value in &values.values {
                    doc(&mut out, "  ", value.docs.as_deref());
                    out.push_str(&format!(
                        "  {}: {},\n",
                        property(&value.name),
                        literal(&value.value)
                    ));
                }
                out.push_str("} as const;\n");
                doc(&mut out, "", values.docs.as_deref());
                out.push_str(&format!(
                    "export type {name} = (typeof {name})[keyof typeof {name}];\n"
                ));
            }
            Shape::List(list) => {
                doc(
                    &mut out,
                    "",
                    crucible_codegen_common::documentation::model(shape).as_deref(),
                );
                let item = ts_type(service, &list.member.target, "");
                let item = if list.sparse {
                    format!("({item} | null)")
                } else {
                    item
                };
                out.push_str(&format!("export type {name} = {item}[];\n"));
            }
            Shape::Map(map) => {
                doc(
                    &mut out,
                    "",
                    crucible_codegen_common::documentation::model(shape).as_deref(),
                );
                let value = ts_type(service, &map.value.target, "");
                let value = if map.sparse {
                    format!("{value} | null")
                } else {
                    value
                };
                let record = match service.shape(&map.key.target) {
                    Some(Shape::Enum(_)) => format!(
                        "globalThis.Partial<globalThis.Record<{}, {value}>>",
                        ts_type(service, &map.key.target, "")
                    ),
                    _ => format!("globalThis.Record<string, {value}>"),
                };
                out.push_str(&format!("export type {name} = {record};\n"));
            }
            Shape::Structure(structure) => {
                doc(&mut out, "", structure.docs.as_deref());
                out.push_str(&format!("export interface {name} {{\n"));
                fields(&mut out, service, structure, "");
                out.push_str("}\n");
            }
            Shape::Union(union) => {
                doc(&mut out, "", union.docs.as_deref());
                out.push_str(&format!("export type {name} =\n"));
                union_arms(&mut out, service, union);
            }
        }
    }
    let mut header = String::from(crate::HEADER);
    header.push_str(&crate::names::runtime_types(&out, &[]));
    header + &out
}

/// Shapes with a type of their own here: everything but the prelude and errors.
pub(crate) fn model_shapes(service: &Service) -> Vec<(&ShapeId, &Shape)> {
    let mut shapes: Vec<(&ShapeId, &Shape)> = service
        .shapes
        .iter()
        .filter(|(id, shape)| {
            !id.is_prelude() && !matches!(shape, Shape::Structure(s) if s.error.is_some())
        })
        .collect();
    shapes.sort_by_key(|(id, _)| service.name_of(id));
    shapes
}

/// Interface members, one per line, each with its docs.
pub(crate) fn fields(out: &mut String, service: &Service, structure: &Structure, prefix: &str) {
    for member in &structure.members {
        doc(out, "  ", member.docs.as_deref());
        let marker = if optional(structure, member) { "?" } else { "" };
        out.push_str(&format!(
            "  {}{marker}: {};\n",
            property(&member.name),
            ts_type(service, &member.target, prefix)
        ));
    }
}

/// One arm per member, each forbidding the others: exactly one is set.
fn union_arms(out: &mut String, service: &Service, union: &Structure) {
    // An event stream's modeled errors are thrown as their error classes,
    // never yielded, and a client cannot send one.
    let members: Vec<_> = union
        .members
        .iter()
        .filter(|member| {
            !(union.streaming
                && matches!(service.shape(&member.target), Some(Shape::Structure(s)) if s.error.is_some()))
        })
        .collect();
    for member in &members {
        let others: String = union
            .members
            .iter()
            .filter(|other| other.name != member.name)
            .map(|other| format!(" {}?: never;", property(&other.name)))
            .collect();
        out.push_str("  | {\n");
        doc(out, "      ", member.docs.as_deref());
        out.push_str(&format!(
            "      {}: {};{others} $unknown?: never;\n    }}\n",
            property(&member.name),
            ts_type(service, &member.target, "")
        ));
    }
    let absent: String = union
        .members
        .iter()
        .map(|member| format!(" {}?: never;", property(&member.name)))
        .collect();
    out.push_str(&format!("  | (UnknownUnion & {{{absent} }})\n"));
    // Close the type alias.
    let trimmed = out.trim_end_matches('\n').len();
    out.truncate(trimmed);
    out.push_str(";\n");
}
