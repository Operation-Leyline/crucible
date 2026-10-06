//! The `model` module: a Rust type per shape, serde attributes for the wire,
//! and `Validate` for every shape an operation input reaches.

use std::collections::{BTreeMap, BTreeSet};

use crucible_smithy::ir::{Constraints, Member, Service, Shape, SimpleKind, Structure};
use crucible_smithy::{Diagnostics, ShapeId};
use serde_json::Value;

use crate::names::{doc, field, function, literal, variant};
use crate::types::{boxed_members, builtin, field_type, rust_type};

/// The body of `pub mod model { ... }`.
/// The structures that refuse unknown members: those marked
/// `@crucible.protocols#strict`, and with `all_inputs` every structure an
/// input reaches.
pub(crate) fn strict_shapes(service: &Service, all_inputs: bool) -> BTreeSet<ShapeId> {
    let marker = ShapeId::new("crucible.protocols", "strict");
    let mut strict: BTreeSet<ShapeId> = service
        .shapes
        .iter()
        .filter_map(|(id, shape)| match shape {
            Shape::Structure(s) if s.traits.contains_key(&marker) => Some(id.clone()),
            _ => None,
        })
        .collect();
    if all_inputs {
        strict.extend(
            input_reachable(service)
                .into_iter()
                .filter(|id| matches!(service.shape(id), Some(Shape::Structure(_)))),
        );
    }
    strict
}

pub(crate) fn emit(
    service: &Service,
    strict: &BTreeSet<ShapeId>,
    client: bool,
    diagnostics: &mut Diagnostics,
) -> String {
    let boxed = boxed_members(service);
    let builders = crate::builders::names(service);
    let validated = input_reachable(service);
    let mut patterns = Patterns::default();
    let mut out = String::new();
    let mut defaults = String::new();
    let mut validations = String::new();

    for (id, shape) in model_shapes(service) {
        let name = service.name_of(id);
        out.push('\n');
        match shape {
            Shape::IntEnum(values) => crate::open::int_enumeration(&mut out, name, values, client),
            Shape::Unit => {}
            Shape::Simple(simple) => {
                doc(&mut out, "", simple.docs.as_deref());
                out.push_str(&format!(
                    "pub type {name} = {};\n",
                    if simple.streaming {
                        "::crucible::ByteStream"
                    } else {
                        builtin(simple.kind)
                    }
                ));
            }
            Shape::List(list) => {
                doc(&mut out, "", list.docs.as_deref());
                let item = rust_type(service, &list.member.target, "");
                let item = if list.sparse {
                    format!("::std::option::Option<{item}>")
                } else {
                    item
                };
                out.push_str(&format!("pub type {name} = ::std::vec::Vec<{item}>;\n"));
            }
            Shape::Map(map) => {
                doc(&mut out, "", map.docs.as_deref());
                let key = rust_type(service, &map.key.target, "");
                let value = rust_type(service, &map.value.target, "");
                let value = if map.sparse {
                    format!("::std::option::Option<{value}>")
                } else {
                    value
                };
                out.push_str(&format!(
                    "pub type {name} = ::std::collections::BTreeMap<{key}, {value}>;\n"
                ));
            }
            Shape::Enum(values) => enumeration(&mut out, id, name, values, client, diagnostics),
            Shape::Structure(structure) => {
                doc(&mut out, "", structure.docs.as_deref());
                out.push_str(crate::DERIVE);
                if strict.contains(id) {
                    out.push_str("#[serde(deny_unknown_fields)]\n");
                }
                out.push_str(&format!("pub struct {name} {{\n"));
                for member in &structure.members {
                    let is_boxed = boxed.contains(&id.with_member(&member.name));
                    member_field(
                        &mut out,
                        service,
                        id,
                        member,
                        is_boxed,
                        "",
                        &mut defaults,
                        client,
                        diagnostics,
                    );
                }
                out.push_str("}\n");
                if client {
                    crate::builders::emit(
                        &mut out,
                        service,
                        id,
                        structure,
                        &boxed,
                        "",
                        &builders[id],
                    );
                }
            }
            Shape::Union(union) if client => {
                crate::open::union(&mut out, service, id, name, union, &boxed)
            }
            Shape::Union(union) => {
                doc(&mut out, "", union.docs.as_deref());
                out.push_str(crate::DERIVE);
                out.push_str(&format!("pub enum {name} {{\n"));
                for member in &union.members {
                    doc(&mut out, "    ", member.docs.as_deref());
                    let mut ty = rust_type(service, &member.target, "");
                    if boxed.contains(&id.with_member(&member.name)) {
                        ty = format!("::std::boxed::Box<{ty}>");
                    }
                    let adapter = if let Some(mut codec) = crate::types::codec(service, member) {
                        if boxed.contains(&id.with_member(&member.name)) {
                            codec = format!("::crucible::codec::Boxed<{codec}>");
                        }
                        format!("#[serde(with = \"::crucible::codec::Adapter::<{codec}>\")] ")
                    } else {
                        String::new()
                    };
                    out.push_str(&format!(
                        "    #[serde(rename = {})]\n    {}({adapter}{ty}),\n",
                        literal(&member.wire_name),
                        variant(&member.name)
                    ));
                }
                out.push_str("}\n");
            }
        }
        if validated.contains(id) {
            if let Shape::Structure(structure) | Shape::Union(structure) = shape {
                validations.push_str(&validate_impl(
                    service,
                    id,
                    structure,
                    shape,
                    client,
                    &mut patterns,
                ));
            }
        }
    }

    let mut code = String::from("use ::crucible::validate::{Validate, Validator};\n");
    code.push_str(&out);
    if !defaults.is_empty() {
        code.push_str("\nmod defaults {\n");
        code.push_str(&defaults);
        code.push_str("}\n");
    }
    code.push_str(&patterns.statics());
    code.push_str(&validations);
    code
}

/// Shapes with a type of their own: everything but the prelude and errors.
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

fn enumeration(
    out: &mut String,
    id: &ShapeId,
    name: &str,
    values: &crucible_smithy::ir::Enum,
    client: bool,
    diagnostics: &mut Diagnostics,
) {
    let mut seen = BTreeSet::new();
    for value in &values.values {
        if !seen.insert(variant(&value.name)) {
            diagnostics.push(
                &id.with_member(&value.name),
                format!(
                    "two values of {name} are both `{}` in Rust",
                    variant(&value.name)
                ),
            );
        }
    }
    if client {
        crate::open::enumeration(out, name, values);
        return;
    }
    doc(out, "", values.docs.as_deref());
    out.push_str(crate::DERIVE_ENUM);
    out.push_str(&format!("pub enum {name} {{\n"));
    for value in &values.values {
        doc(out, "    ", value.docs.as_deref());
        out.push_str(&format!(
            "    #[serde(rename = {})]\n    {},\n",
            literal(&value.value),
            variant(&value.name)
        ));
    }
    out.push_str("}\n\n");
    out.push_str(&format!(
        "impl {name} {{\n    /// The value on the wire.\n    pub fn as_str(&self) -> &'static str {{\n        match self {{\n"
    ));
    for value in &values.values {
        out.push_str(&format!(
            "            Self::{} => {},\n",
            variant(&value.name),
            literal(&value.value)
        ));
    }
    out.push_str("        }\n    }\n}\n");
}

/// One public struct field with its serde attributes. Every member is
/// serialized; the protocol moves bound members out of the body. `prefix`
/// names model types from where the struct is (`super::model::` in errors).
#[allow(clippy::too_many_arguments)]
pub(crate) fn member_field(
    out: &mut String,
    service: &Service,
    owner: &ShapeId,
    member: &Member,
    boxed: bool,
    prefix: &str,
    defaults: &mut String,
    client: bool,
    diagnostics: &mut Diagnostics,
) {
    doc(out, "    ", member.docs.as_deref());
    let mut serde = vec![format!("rename = {}", literal(&member.wire_name))];
    let optional = if client {
        member.optional_on_client()
    } else {
        member.optional_on_server()
    };
    let default = if client {
        member.client_default()
    } else {
        member.default.as_ref()
    };
    // A protocol that omits null structure members never sends an explicit
    // null and reads one as absence; documents keep their nulls.
    let omit_null = client
        && optional
        && service.protocol_settings.omit_null_structure_members
        && crate::types::simple_kind(service, &member.target) != Some(SimpleKind::Document);
    if optional {
        if default.is_none() {
            serde.push("default".to_owned());
        }
        serde.push(
            if omit_null {
                "skip_serializing_if = \"::crucible::Field::is_unset\""
            } else if client {
                "skip_serializing_if = \"::crucible::Field::is_absent\""
            } else {
                "skip_serializing_if = \"::std::option::Option::is_none\""
            }
            .to_owned(),
        );
    }
    if let Some(value) = default {
        let inner = format!("super::{prefix}");
        match default_expr(service, member, value, &inner) {
            Some(expr) => {
                let expr = if client && optional {
                    format!("::crucible::Field::Value({expr})")
                } else {
                    expr
                };
                let function = format!(
                    "{}__{}",
                    function(service.name_of(owner)),
                    function(&member.name).trim_start_matches("r#")
                );
                let ty = field_type(service, member, boxed, &inner, client);
                defaults.push_str(&format!(
                    "    pub(super) fn {function}() -> {ty} {{\n        {expr}\n    }}\n"
                ));
                serde.push(format!("default = \"defaults::{function}\""));
            }
            None => diagnostics.push(
                &owner.with_member(&member.name),
                "this default value is not generated yet",
            ),
        }
    }
    if client
        && !optional
        && crate::types::simple_kind(service, &member.target) == Some(SimpleKind::Document)
    {
        serde.push("deserialize_with = \"::crucible::field::required\"".to_owned());
    }
    if let Some(mut codec) = crate::types::codec(service, member) {
        if boxed {
            codec = format!("::crucible::codec::Boxed<{codec}>");
        }
        if optional {
            codec = format!(
                "::crucible::codec::{}<{codec}>",
                if omit_null {
                    "NullAbsent"
                } else if client {
                    "Field"
                } else {
                    "Optional"
                }
            );
        }
        serde.push(format!("with = \"::crucible::codec::Adapter::<{codec}>\""));
    } else if omit_null {
        serde.push(
            "with = \"::crucible::codec::Adapter::<::crucible::codec::NullAbsent<::crucible::codec::Identity>>\""
                .to_owned(),
        );
    }
    out.push_str(&format!("    #[serde({})]\n", serde.join(", ")));
    out.push_str(&format!(
        "    pub {}: {},\n",
        field(&member.name),
        field_type(service, member, boxed, prefix, client)
    ));
}

/// A default value as Rust, in a `defaults` module that names model types
/// with `prefix`.
fn default_expr(service: &Service, member: &Member, value: &Value, prefix: &str) -> Option<String> {
    match (service.shape(&member.target)?, value) {
        (Shape::IntEnum(values), Value::Number(number)) => {
            let value = values
                .values
                .iter()
                .find(|v| Some(i64::from(v.value)) == number.as_i64())?;
            Some(format!(
                "{prefix}{}::{}",
                service.name_of(&member.target),
                variant(&value.name)
            ))
        }
        (Shape::Enum(values), Value::String(text)) => {
            let value = values.values.iter().find(|v| v.value == *text)?;
            Some(format!(
                "{prefix}{}::{}",
                service.name_of(&member.target),
                variant(&value.name)
            ))
        }
        (Shape::Simple(simple), _) => match (simple.kind, value) {
            (SimpleKind::Boolean, Value::Bool(b)) => Some(b.to_string()),
            (SimpleKind::String, Value::String(s)) => Some(format!("{}.to_owned()", literal(s))),
            (SimpleKind::Float | SimpleKind::Double, Value::Number(n)) => {
                let text = n.to_string();
                Some(if text.contains(['.', 'e', 'E']) {
                    text
                } else {
                    format!("{text}.0")
                })
            }
            (
                SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long,
                Value::Number(n),
            ) => Some(n.to_string()),
            (SimpleKind::BigInteger | SimpleKind::BigDecimal, Value::Number(number)) => {
                Some(format!(
                    "{}.parse().expect(\"validated numeric default\")",
                    literal(&number.to_string())
                ))
            }
            (SimpleKind::Timestamp, Value::Number(number)) => Some(format!(
                "::crucible::time::epoch_seconds::parse({}).expect(\"validated timestamp default\")",
                literal(&number.to_string())
            )),
            (SimpleKind::Timestamp, Value::String(text)) => Some(format!(
                "::crucible::time::parse_date_time({}).expect(\"validated timestamp default\")",
                literal(text)
            )),
            (SimpleKind::Document, value) => {
                Some(format!("::crucible::__private::serde_json::json!({value})"))
            }
            _ => None,
        },
        (Shape::List(_) | Shape::Map(_), Value::Array(a)) if a.is_empty() => {
            Some("::std::default::Default::default()".to_owned())
        }
        (Shape::Map(_), Value::Object(o)) if o.is_empty() => {
            Some("::std::default::Default::default()".to_owned())
        }
        _ => None,
    }
}

/// Structures and unions an operation input reaches: what the server validates.
fn input_reachable(service: &Service) -> BTreeSet<ShapeId> {
    let mut reached = BTreeSet::new();
    let mut stack: Vec<ShapeId> = service
        .operations
        .iter()
        .filter_map(|op| op.input.clone())
        .collect();
    while let Some(id) = stack.pop() {
        if !reached.insert(id.clone()) {
            continue;
        }
        let targets: Vec<&ShapeId> = match service.shape(&id) {
            Some(Shape::Structure(s) | Shape::Union(s)) => {
                s.members.iter().map(|m| &m.target).collect()
            }
            Some(Shape::List(l)) => vec![&l.member.target],
            Some(Shape::Map(m)) => vec![&m.key.target, &m.value.target],
            _ => vec![],
        };
        stack.extend(targets.into_iter().cloned());
    }
    reached
}

/// Regexes for `@pattern`, one static each, compiled on first use.
#[derive(Default)]
struct Patterns {
    names: BTreeMap<String, String>,
}

impl Patterns {
    fn name(&mut self, pattern: &str) -> String {
        let next = format!("PATTERN_{}", self.names.len());
        self.names.entry(pattern.to_owned()).or_insert(next).clone()
    }

    fn statics(&self) -> String {
        let mut out = String::new();
        let mut entries: Vec<(&String, &String)> = self.names.iter().collect();
        entries.sort_by_key(|(_, name)| (name.len(), *name));
        for (pattern, name) in entries {
            out.push_str(&format!(
                "\nstatic {name}: ::std::sync::LazyLock<::crucible::__private::regex::Regex> =\n    ::std::sync::LazyLock::new(|| ::crucible::__private::regex::Regex::new({}).expect(\"checked by crucible\"));\n",
                literal(pattern)
            ));
        }
        out
    }
}

fn validate_impl(
    service: &Service,
    id: &ShapeId,
    structure: &Structure,
    shape: &Shape,
    client: bool,
    patterns: &mut Patterns,
) -> String {
    let name = service.name_of(id);
    let mut body = String::new();
    match shape {
        Shape::Union(_) => {
            body.push_str("        match self {\n");
            for member in &structure.members {
                let checks = value_checks(service, member, "value", patterns, 4);
                let binding = if checks.is_empty() { "_" } else { "value" };
                body.push_str(&format!(
                    "            Self::{}({binding}) => {{\n",
                    variant(&member.name)
                ));
                if !checks.is_empty() {
                    body.push_str(&format!(
                        "                validator.at({}, |validator| {{\n{checks}                }});\n",
                        literal(&member.name)
                    ));
                }
                body.push_str("            }\n");
            }
            if client {
                let unknown =
                    crate::open::unknown_variant(structure.members.iter().map(|m| m.name.as_str()));
                body.push_str(&format!("            Self::{unknown}(_) => {{}}\n"));
            }
            body.push_str("        }\n");
        }
        _ => {
            for member in &structure.members {
                let checks = value_checks(service, member, "value", patterns, 3);
                if checks.is_empty() {
                    continue;
                }
                let access = if client && member.optional_on_client() {
                    format!(
                        "if let ::crucible::Field::Value(value) = &self.{} {{\n",
                        field(&member.name)
                    )
                } else if !client && member.optional_on_server() {
                    format!("if let Some(value) = &self.{} {{\n", field(&member.name))
                } else {
                    format!(
                        "{{\n            let value = &self.{};\n",
                        field(&member.name)
                    )
                };
                body.push_str(&format!(
                    "        {access}            validator.at({}, |validator| {{\n{checks}            }});\n        }}\n",
                    literal(&member.name)
                ));
            }
        }
    }
    if body.is_empty() {
        body.push_str("        let _ = validator;\n");
    }
    format!(
        "\nimpl Validate for {name} {{\n    fn validate(&self, validator: &mut Validator) {{\n{body}    }}\n}}\n"
    )
}

/// Checks on `value` (a reference to the member's type, unboxed by auto-deref)
/// for the member's constraints and its target's members, indented `depth` levels.
fn value_checks(
    service: &Service,
    member: &Member,
    value: &str,
    patterns: &mut Patterns,
    depth: usize,
) -> String {
    let pad = "    ".repeat(depth + 1);
    let mut out = String::new();
    let c: &Constraints = &member.constraints;
    match service.shape(&member.target) {
        Some(Shape::Simple(simple)) => {
            if simple.kind == SimpleKind::String {
                if let Some(length) = &c.length {
                    out.push_str(&format!(
                        "{pad}validator.length({value}.chars().count(), {}, {});\n",
                        opt(length.min),
                        opt(length.max)
                    ));
                }
                if let Some(pattern) = &c.pattern {
                    let name = patterns.name(pattern);
                    out.push_str(&format!(
                        "{pad}validator.pattern({value}, &{name}, {});\n",
                        literal(pattern)
                    ));
                }
            }
            // A blob's length is its byte count.
            if simple.kind == SimpleKind::Blob {
                if let Some(length) = &c.length {
                    if simple.streaming {
                        out.push_str(&format!("{pad}if let Some(length) = {value}.length() {{ validator.length(length, {}, {}); }}\n", opt(length.min), opt(length.max)));
                    } else {
                        out.push_str(&format!(
                            "{pad}validator.length({value}.len(), {}, {});\n",
                            opt(length.min),
                            opt(length.max)
                        ));
                    }
                }
            }
            if let Some(range) = &c.range {
                out.push_str(&range_check(simple.kind, value, range, &pad));
            }
        }
        Some(Shape::List(list)) => {
            if let Some(length) = &c.length {
                out.push_str(&format!(
                    "{pad}validator.length({value}.len(), {}, {});\n",
                    opt(length.min),
                    opt(length.max)
                ));
            }
            if list.is_set || c.unique_items {
                out.push_str(&format!("{pad}validator.unique({value});\n"));
            }
            let item = value_checks(service, &list.member, "item", patterns, depth + 2);
            let item = if list.sparse && !item.is_empty() {
                format!("{pad}        if let Some(item) = item {{\n{item}{pad}        }}\n")
            } else {
                item
            };
            if !item.is_empty() {
                out.push_str(&format!(
                    "{pad}for (index, item) in {value}.iter().enumerate() {{\n{pad}    validator.at(index, |validator| {{\n{item}{pad}    }});\n{pad}}}\n"
                ));
            }
        }
        Some(Shape::Map(map)) => {
            if let Some(length) = &c.length {
                out.push_str(&format!(
                    "{pad}validator.length({value}.len(), {}, {});\n",
                    opt(length.min),
                    opt(length.max)
                ));
            }
            let key = value_checks(service, &map.key, "key", patterns, depth + 2);
            let item = value_checks(service, &map.value, "item", patterns, depth + 2);
            let item = if map.sparse && !item.is_empty() {
                format!("{pad}        if let Some(item) = item {{\n{item}{pad}        }}\n")
            } else {
                item
            };
            if !key.is_empty() || !item.is_empty() {
                let item_binding = if item.is_empty() { "_item" } else { "item" };
                // The path names the key; an enum key spells itself as on the wire.
                let path = match service.shape(&map.key.target) {
                    Some(Shape::Enum(_)) => "key.as_str()",
                    _ => "key",
                };
                out.push_str(&format!(
                    "{pad}for (key, {item_binding}) in {value} {{\n{pad}    validator.at({path}, |validator| {{\n{key}{item}{pad}    }});\n{pad}}}\n"
                ));
            }
        }
        // Events are not validated as they are sent; the stream handle has nothing to check.
        Some(Shape::Union(union)) if union.streaming => {}
        Some(Shape::Structure(_) | Shape::Union(_)) => {
            out.push_str(&format!("{pad}{value}.validate(validator);\n"));
        }
        _ => {}
    }
    out
}

fn range_check(
    kind: SimpleKind,
    value: &str,
    range: &crucible_smithy::ir::Bounds<serde_json::Number>,
    pad: &str,
) -> String {
    if matches!(kind, SimpleKind::BigInteger | SimpleKind::BigDecimal) {
        let bound = |name: &str, number: &Option<serde_json::Number>| {
            number.as_ref().map_or_else(
                || "None".to_owned(),
                |number| {
                    format!(
                        "Some({name}.get_or_init(|| {}.parse().expect(\"validated range bound\")))",
                        literal(&number.to_string())
                    )
                },
            )
        };
        let statics = [("MIN", &range.min), ("MAX", &range.max)].into_iter().filter(|(_, value)| value.is_some()).map(|(name, _)| format!("{pad}    static {name}: ::std::sync::OnceLock<::crucible::number::DecimalValue> = ::std::sync::OnceLock::new();\n")).collect::<String>();
        return format!(
            "{pad}{{\n{statics}{pad}    validator.range_decimal(&{value}.as_decimal(), {}, {});\n{pad}}}\n",
            bound("MIN", &range.min),
            bound("MAX", &range.max)
        );
    }
    let float = matches!(kind, SimpleKind::Float | SimpleKind::Double);
    let integral = |n: &serde_json::Number| {
        n.as_i64()
            .map(|i| i.to_string())
            .or_else(|| n.as_u64().map(|u| u.to_string()))
    };
    if !float
        && range
            .min
            .iter()
            .chain(&range.max)
            .all(|n| integral(n).is_some())
    {
        let bound = |n: &Option<serde_json::Number>| match n {
            Some(n) => format!("Some({}i128)", integral(n).expect("checked integral")),
            None => "None".to_owned(),
        };
        format!(
            "{pad}validator.range_integer(i128::from(*{value}), {}, {});\n",
            bound(&range.min),
            bound(&range.max)
        )
    } else {
        let bound = |n: &Option<serde_json::Number>| match n {
            Some(n) => format!("Some({}f64)", n.as_f64().unwrap_or_default()),
            None => "None".to_owned(),
        };
        format!(
            "{pad}validator.range_float(*{value} as f64, {}, {});\n",
            bound(&range.min),
            bound(&range.max)
        )
    }
}

fn opt(value: Option<u64>) -> String {
    value.map_or("None".to_owned(), |v| format!("Some({v})"))
}
