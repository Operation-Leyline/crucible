//! `schemas.ts`: what the runtime needs to put each operation on the wire.
//! Internal to the package; `index.ts` does not export it.

use std::collections::BTreeSet;

use crucible_core::ShapeId;
use crucible_core::ir::{Binding, Member, Operation, Service, Shape, SimpleKind, Structure};

use crate::errors::error_shapes;
use crate::names::{literal, property};

pub(crate) fn emit(service: &Service) -> String {
    let converting = converting(service);
    let mut out = String::new();
    let mut structures: Vec<&ShapeId> = converting.iter().collect();
    structures.sort_by_key(|id| service.name_of(id));
    for id in structures {
        let Some(Shape::Structure(structure) | Shape::Union(structure)) = service.shape(id) else {
            continue;
        };
        let members: Vec<String> = structure
            .members
            .iter()
            .filter_map(|member| {
                body_schema(service, &converting, member)
                    .map(|schema| format!("{}: {schema}", property(&member.name)))
            })
            .collect();
        let defaults: Vec<String> = structure
            .members
            .iter()
            .filter_map(|member| {
                let value = member.default.as_ref()?;
                Some(format!("{}: {value}", property(&member.name)))
            })
            .collect();
        let defaults = if defaults.is_empty() {
            String::new()
        } else {
            format!(", {{ {} }}", defaults.join(", "))
        };
        out.push_str(&format!(
            "\nexport const {}: Schema = structure(() => ({{ {} }}){defaults});\n",
            schema_name(service, id),
            members.join(", ")
        ));
    }

    for operation in &service.operations {
        out.push('\n');
        out.push_str(&operation_spec(service, &converting, operation));
    }
    out.push_str(&error_table(service, &converting));

    // Import only the helpers this model uses, so no import is ever unused.
    let helpers: Vec<&str> = [
        "boolean",
        "list",
        "map",
        "number",
        "string",
        "structure",
        "timestamp",
    ]
    .into_iter()
    .filter(|helper| {
        out.contains(&format!("{helper}("))
            || out.contains(&format!(": {helper}"))
            || out.contains(&format!(", {helper}]"))
            || out.contains(&format!("({helper})"))
    })
    .collect();
    let mut header = String::from(crate::HEADER);
    if !helpers.is_empty() {
        header.push_str(&format!(
            "import {{ {} }} from \"./runtime.js\";\n",
            helpers.join(", ")
        ));
    }
    header.push_str("import type { ErrorSpec, OperationSpec, Schema } from \"./runtime.js\";\n");
    if out.contains("new e.") {
        header.push_str("import * as e from \"./errors.js\";\n");
    }
    header + &out
}

pub(crate) fn schema_name(service: &Service, id: &ShapeId) -> String {
    format!("S_{}", service.name_of(id))
}

/// Structures and unions whose wire form differs from their model form at any
/// depth: they hold a timestamp, or a member whose default the client fills.
pub(crate) fn converting(service: &Service) -> BTreeSet<ShapeId> {
    let mut set = BTreeSet::new();
    loop {
        let before = set.len();
        for (id, shape) in &service.shapes {
            if let Shape::Structure(structure) | Shape::Union(structure) = shape {
                let differs = |m: &Member| m.default.is_some() || needs(service, &set, &m.target);
                if structure.members.iter().any(differs) {
                    set.insert(id.clone());
                }
            }
        }
        if set.len() == before {
            return set;
        }
    }
}

fn needs(service: &Service, set: &BTreeSet<ShapeId>, target: &ShapeId) -> bool {
    match service.shape(target) {
        Some(Shape::Simple(simple)) => simple.kind == SimpleKind::Timestamp,
        Some(Shape::List(list)) => needs(service, set, &list.member.target),
        Some(Shape::Map(map)) => needs(service, set, &map.value.target),
        Some(Shape::Structure(_) | Shape::Union(_)) => set.contains(target),
        _ => false,
    }
}

/// The schema of a member in a JSON body, or `None` when it passes through.
pub(crate) fn body_schema(
    service: &Service,
    converting: &BTreeSet<ShapeId>,
    member: &Member,
) -> Option<String> {
    match service.shape(&member.target)? {
        Shape::Simple(simple) if simple.kind == SimpleKind::Timestamp => {
            Some(timestamp(service, member))
        }
        Shape::List(list) => {
            body_schema(service, converting, &list.member).map(|item| format!("list({item})"))
        }
        Shape::Map(map) => {
            body_schema(service, converting, &map.value).map(|value| format!("map({value})"))
        }
        Shape::Structure(_) | Shape::Union(_) if converting.contains(&member.target) => {
            Some(schema_name(service, &member.target))
        }
        _ => None,
    }
}

/// The schema of a member bound to a label, query parameter or header. Text
/// needs none; numbers, booleans and timestamps say how to read them back.
fn binding_schema(service: &Service, member: &Member) -> Option<String> {
    match service.shape(&member.target)? {
        Shape::Simple(simple) => match simple.kind {
            SimpleKind::Timestamp => Some(timestamp(service, member)),
            SimpleKind::Boolean => Some("boolean".to_owned()),
            SimpleKind::String | SimpleKind::Document => None,
            _ => Some("number".to_owned()),
        },
        Shape::List(list) => Some(format!(
            "list({})",
            binding_schema(service, &list.member).unwrap_or_else(|| "string".to_owned())
        )),
        _ => None,
    }
}

fn timestamp(service: &Service, member: &Member) -> String {
    let format = match service.timestamp_format(member) {
        crucible_core::ir::TimestampFormat::DateTime => "date-time",
        crucible_core::ir::TimestampFormat::HttpDate => "http-date",
        crucible_core::ir::TimestampFormat::EpochSeconds => "epoch-seconds",
    };
    format!("timestamp({})", literal(format))
}

/// `[member, name, schema?]` tuples for bound members.
pub(crate) fn bindings(service: &Service, structure: &Structure, bindings: &[Binding]) -> String {
    let tuples: Vec<String> = bindings
        .iter()
        .map(|binding| {
            let schema = structure
                .member(&binding.member)
                .and_then(|member| binding_schema(service, member))
                .map(|schema| format!(", {schema}"))
                .unwrap_or_default();
            format!(
                "[{}, {}{schema}]",
                literal(&binding.member),
                literal(&binding.name)
            )
        })
        .collect();
    format!("[{}]", tuples.join(", "))
}

fn structure<'s>(service: &'s Service, id: Option<&ShapeId>) -> Option<&'s Structure> {
    match service.shape(id?)? {
        Shape::Structure(structure) => Some(structure),
        _ => None,
    }
}

fn operation_spec(service: &Service, converting: &BTreeSet<ShapeId>, op: &Operation) -> String {
    let mut fields = vec![
        format!("name: {}", literal(op.name())),
        format!("method: {}", literal(&op.method)),
    ];
    let input = structure(service, op.input.as_ref());
    let path: Vec<String> = op
        .uri
        .segments
        .iter()
        .map(|segment| match segment {
            crucible_core::ir::Segment::Literal(text) => literal(text),
            crucible_core::ir::Segment::Label(label) => {
                let schema = input
                    .and_then(|input| input.member(label))
                    .and_then(|member| binding_schema(service, member))
                    .map(|schema| format!(", schema: {schema}"))
                    .unwrap_or_default();
                format!("{{ label: {}{schema} }}", literal(label))
            }
        })
        .collect();
    fields.push(format!("path: [{}]", path.join(", ")));
    if !op.uri.query.is_empty() {
        let pairs: Vec<String> = op
            .uri
            .query
            .iter()
            .map(|(key, value)| {
                let value = value.as_deref().map_or("undefined".to_owned(), literal);
                format!("[{}, {value}]", literal(key))
            })
            .collect();
        fields.push(format!("literalQuery: [{}]", pairs.join(", ")));
    }
    if let Some(input) = input {
        if !op.request.query.is_empty() {
            fields.push(format!(
                "query: {}",
                bindings(service, input, &op.request.query)
            ));
        }
        if !op.request.headers.is_empty() {
            fields.push(format!(
                "headers: {}",
                bindings(service, input, &op.request.headers)
            ));
        }
        if !op.request.body.is_empty() {
            let body: Vec<String> = op.request.body.iter().map(|m| literal(m)).collect();
            fields.push(format!("body: [{}]", body.join(", ")));
            if let Some(id) = op.input.as_ref().filter(|id| converting.contains(*id)) {
                fields.push(format!("input: {}", schema_name(service, id)));
            }
        }
    }
    if let (Some(id), Some(output)) = (op.output.as_ref(), structure(service, op.output.as_ref())) {
        let mut parts = vec![format!("body: {}", !op.response.body.is_empty())];
        if converting.contains(id) {
            parts.push(format!("schema: {}", schema_name(service, id)));
        }
        if !op.response.headers.is_empty() {
            parts.push(format!(
                "headers: {}",
                bindings(service, output, &op.response.headers)
            ));
        }
        fields.push(format!("output: {{ {} }}", parts.join(", ")));
    }
    let errors: Vec<String> = op
        .errors
        .iter()
        .map(|id| literal(service.name_of(id)))
        .collect();
    fields.push(format!("errors: [{}]", errors.join(", ")));
    if let Some(token) = &op.idempotency_token {
        fields.push(format!("idempotencyToken: {}", literal(token)));
    }
    let retry_safe = op.readonly || op.idempotent || op.idempotency_token.is_some();
    fields.push(format!("retrySafe: {retry_safe}"));

    format!(
        "export const {}: OperationSpec = {{\n  {},\n}};\n",
        op.name(),
        fields.join(",\n  ")
    )
}

/// The errors the transport can decode, by shape name.
fn error_table(service: &Service, converting: &BTreeSet<ShapeId>) -> String {
    let mut out = String::from("\nexport const errors: Readonly<Record<string, ErrorSpec>> = {\n");
    for (id, structure) in error_shapes(service) {
        let error = structure.error.as_ref().expect("filtered to errors");
        let name = service.name_of(id);
        let mut fields = vec![format!("status: {}", error.status)];
        if converting.contains(id) {
            fields.push(format!("schema: {}", schema_name(service, id)));
        }
        if !error.headers.is_empty() {
            fields.push(format!(
                "headers: {}",
                bindings(service, structure, &error.headers)
            ));
        }
        fields.push(format!(
            "create: (fields, metadata) => new e.{name}(fields, metadata)"
        ));
        out.push_str(&format!("  {name}: {{ {} }},\n", fields.join(", ")));
    }
    out.push_str("};\n");
    out
}
