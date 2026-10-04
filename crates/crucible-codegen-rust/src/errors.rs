//! `errors.rs`: a struct per modelled error, an enum per operation of the
//! errors it can return, and the validation error a rejected request becomes.

use crucible_smithy::ir::{Fault, Member, Service, Shape, SimpleKind, Structure};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::models::member_field;
use crate::names::{doc, field, literal};
use crate::types::{boxed_members, simple_kind};

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

pub(crate) fn emit(
    service: &Service,
    validation: &ShapeId,
    diagnostics: &mut Diagnostics,
) -> String {
    let boxed = boxed_members(service);
    let mut out = String::from(crate::HEADER);
    out.push_str("#![allow(clippy::all)]\n\n");
    out.push_str("use crate::runtime::{ModelledError, Rejection, ToText};\n");
    let mut defaults = String::new();

    for (id, structure) in error_shapes(service) {
        let name = service.name_of(id);
        let error = structure.error.as_ref().expect("filtered to errors");
        out.push('\n');
        doc(&mut out, "", structure.docs.as_deref());
        out.push_str(
            "#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]\n",
        );
        out.push_str(&format!("pub struct {name} {{\n"));
        for member in &structure.members {
            let is_boxed = boxed.contains(&id.with_member(&member.name));
            member_field(
                &mut out,
                service,
                id,
                member,
                is_boxed,
                "pub ",
                "crate::models::",
                &mut defaults,
                diagnostics,
            );
        }
        out.push_str("}\n\n");
        out.push_str(&format!(
            "impl ModelledError for {name} {{\n    const TYPE: &'static str = {};\n    const STATUS: u16 = {};\n",
            literal(name),
            error.status
        ));
        if !error.headers.is_empty() {
            out.push_str("\n    fn headers(&self) -> ::std::vec::Vec<(&'static str, ::std::string::String)> {\n        let mut headers = ::std::vec::Vec::new();\n");
            for binding in &error.headers {
                if let Some(member) = structure.member(&binding.member) {
                    out.push_str(&header_push(
                        service,
                        member,
                        "self",
                        &binding.name,
                        "        ",
                    ));
                }
            }
            out.push_str("        headers\n    }\n");
        }
        out.push_str("}\n\n");
        let message = structure
            .member("message")
            .filter(|m| simple_kind(service, &m.target) == Some(SimpleKind::String));
        let display = match message {
            Some(m) if m.optional_on_server() => {
                "self.message.as_deref().unwrap_or_default()".to_owned()
            }
            Some(_) => "&self.message".to_owned(),
            None => literal(name),
        };
        out.push_str(&format!(
            "impl ::std::fmt::Display for {name} {{\n    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        write!(f, \"{{}}: {{}}\", {}, {display})\n    }}\n}}\n\n",
            literal(name)
        ));
        out.push_str(&format!("impl ::std::error::Error for {name} {{}}\n"));
    }

    for operation in &service.operations {
        let name = format!("{}Error", operation.name());
        out.push_str(&format!("\n/// The errors {} can return.\n#[derive(Debug, Clone, PartialEq)]\npub enum {name} {{\n", operation.name()));
        for error in &operation.errors {
            let error = service.name_of(error);
            out.push_str(&format!("    {error}({error}),\n"));
        }
        out.push_str("}\n\n");
        for error in &operation.errors {
            let error = service.name_of(error);
            out.push_str(&format!(
                "impl ::std::convert::From<{error}> for {name} {{\n    fn from(error: {error}) -> Self {{\n        Self::{error}(error)\n    }}\n}}\n\n"
            ));
        }
        out.push_str(&format!(
            "impl {name} {{\n    pub(crate) fn into_response(self) -> ::axum::response::Response {{\n        match self {{\n"
        ));
        for error in &operation.errors {
            let error = service.name_of(error);
            out.push_str(&format!(
                "            Self::{error}(error) => crate::runtime::error_response(&error),\n"
            ));
        }
        if operation.errors.is_empty() {
            out.push_str("            _ => unreachable!(),\n");
        }
        out.push_str("        }\n    }\n}\n");
        if !operation.errors.contains(validation) {
            diagnostics.push(
                &operation.id,
                format!("{validation} must be one of the operation's errors: a rejected request becomes it"),
            );
        }
    }

    match service.shape(validation) {
        Some(Shape::Structure(structure))
            if structure
                .error
                .as_ref()
                .is_some_and(|e| e.fault == Fault::Client) => {}
        _ => diagnostics.push(
            validation,
            "the validation error must be a client @error the service reaches",
        ),
    }
    if !defaults.is_empty() {
        out.push_str("\nmod defaults {\n");
        out.push_str(&defaults);
        out.push_str("}\n");
    }
    out
}

/// A response header for `member` of `owner`, pushed onto `headers`.
pub(crate) fn header_push(
    service: &Service,
    member: &Member,
    owner: &str,
    header: &str,
    pad: &str,
) -> String {
    let value = format!("{owner}.{}", field(&member.name));
    let text = text_expr(service, member, "value");
    if member.optional_on_server() {
        format!(
            "{pad}if let Some(value) = &{value} {{\n{pad}    headers.push(({}, {text}));\n{pad}}}\n",
            literal(header)
        )
    } else {
        format!(
            "{pad}{{\n{pad}    let value = &{value};\n{pad}    headers.push(({}, {text}));\n{pad}}}\n",
            literal(header)
        )
    }
}

/// `value` (a reference to the member's type) as header text.
fn text_expr(service: &Service, member: &Member, value: &str) -> String {
    match service.shape(&member.target) {
        Some(Shape::List(list)) => {
            let item = text_expr(service, &list.member, "item");
            format!("crate::runtime::join_header({value}.iter().map(|item| {item}))")
        }
        _ => match crate::types::timestamp_module(service, member) {
            Some("crate::runtime::epoch_seconds") => {
                format!("crate::runtime::epoch_seconds::to_f64({value}).to_string()")
            }
            Some(_) => format!("crate::runtime::http_date::format({value})"),
            None => format!("ToText::to_text({value})"),
        },
    }
}

/// The body of a default `Reject::reject`: the validation error built from a
/// rejection, when crucible can fill every required member. It fills
/// `message` with a summary and `fieldList` (structures with `path` or `name`,
/// and `message`) with every violation, up to the list's `@length` maximum.
/// `None` when the error needs more, such as a required `reason`: the service
/// then implements `reject` itself.
pub(crate) fn default_rejection(service: &Service, validation: &ShapeId) -> Option<String> {
    let Some(Shape::Structure(structure)) = service.shape(validation) else {
        return None;
    };
    let mut fields = Vec::new();
    for member in &structure.members {
        let target = simple_kind(service, &member.target);
        let value = match member.name.as_str() {
            "message" if target == Some(SimpleKind::String) => {
                Some("rejection.summary()".to_owned())
            }
            "fieldList" => field_list(service, member),
            _ => None,
        };
        let rust = field(&member.name);
        match (value, member.optional_on_server()) {
            (Some(value), true) => fields.push(format!("{rust}: Some({value})")),
            (Some(value), false) => fields.push(format!("{rust}: {value}")),
            (None, true) => fields.push(format!("{rust}: None")),
            (None, false) => return None,
        }
    }
    Some(format!(
        "crate::errors::{} {{\n            {},\n        }}",
        service.name_of(validation),
        fields.join(",\n            ")
    ))
}

fn field_list(service: &Service, member: &Member) -> Option<String> {
    let Some(Shape::List(list)) = service.shape(&member.target) else {
        return None;
    };
    let Some(Shape::Structure(item)) = service.shape(&list.member.target) else {
        return None;
    };
    let mut fields = Vec::new();
    for field_member in &item.members {
        let value = match field_member.name.as_str() {
            "path" | "name" => "violation.path.clone()",
            "message" => "violation.message.clone()",
            _ if field_member.optional_on_server() => "None",
            _ => return None,
        };
        let value = if field_member.optional_on_server() && value != "None" {
            format!("Some({value})")
        } else {
            value.to_owned()
        };
        fields.push(format!("{}: {value}", field(&field_member.name)));
    }
    let cap = member
        .constraints
        .length
        .as_ref()
        .and_then(|length| length.max)
        .map(|max| format!(".take({max})"))
        .unwrap_or_default();
    Some(format!(
        "rejection.violations.iter(){cap}.map(|violation| crate::models::{} {{ {} }}).collect()",
        service.name_of(&list.member.target),
        fields.join(", ")
    ))
}
