//! The `errors` module: a struct per modelled error, implementing
//! `crucible_server_runtime_rust::ModelledError`, and the service-wide error functions the
//! framework calls (classify, reject, internal).

use crucible_smithy::ir::{Fault, Member, Service, Shape, SimpleKind, Structure};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::info::{constant, error_bindings, members};
use crate::models::member_field;
use crate::names::{check_distinct, documented, field, literal, variant};
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

/// The body of `pub mod errors { ... }`.
pub(crate) fn emit(service: &Service, client: bool, diagnostics: &mut Diagnostics) -> String {
    let boxed = boxed_members(service);
    let builders = crate::builders::names(service);
    let mut out = String::from("use super::__redaction;\n");
    let mut defaults = String::new();
    for (id, structure) in error_shapes(service) {
        let name = service.name_of(id);
        let error = structure.error.as_ref().expect("filtered to errors");
        check_distinct(
            id,
            structure.members.iter().map(|m| m.name.as_str()),
            field,
            "field",
            diagnostics,
        );
        out.push('\n');
        documented(&mut out, "", structure.docs.as_deref(), &structure.notes);
        out.push_str(crate::DERIVE);
        if client {
            out.push_str(crate::NON_EXHAUSTIVE);
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
                "super::model::",
                &mut defaults,
                client,
                diagnostics,
            );
        }
        out.push_str("}\n\n");
        crate::debug::structure(&mut out, service, name, structure);
        if client {
            crate::builders::emit(
                &mut out,
                service,
                id,
                structure,
                &boxed,
                "super::model::",
                &builders[id],
            );
        }
        let fault = match error.fault {
            Fault::Client => "Client",
            Fault::Server => "Server",
        };
        let throttling = error.retryable.is_some_and(|r| r.throttling);
        let retryable = error.retryable.is_some();
        out.push_str(&format!(
            "pub(super) static {}: ::crucible_server_runtime_rust::__generated::operation::ErrorInfo = ::crucible_server_runtime_rust::__generated::error_info(\n    {},\n    {},\n    ::crucible_server_runtime_rust::__generated::operation::Fault::{fault},\n    {retryable},\n    {throttling},\n    {},\n);\n\n",
            info_static(name),
            literal(&error.wire_code),
            error.status,
            members(service, structure, &error_bindings(error))
        ));
        let status = error.response_code.as_ref().and_then(|name| structure.member(name)).map_or(String::new(), |member| {
            let rust = crate::names::field(&member.name);
            let get = if client && member.optional_on_client() { format!("self.{rust}.value().map(|value| ::crucible_server_runtime_rust::__generated::operation::response_status(i64::from(*value))).unwrap_or({})", error.status) } else if !client && member.optional_on_server() { format!("self.{rust}.as_ref().map(|value| ::crucible_server_runtime_rust::__generated::operation::response_status(i64::from(*value))).unwrap_or({})", error.status) } else { format!("::crucible_server_runtime_rust::__generated::operation::response_status(i64::from(self.{rust}))") };
            format!("    fn response_status(&self) -> u16 {{ {get} }}\n\n")
        });
        out.push_str(&format!(
            "impl ::crucible_server_runtime_rust::__generated::ModelledError for {name} {{\n    fn shape_id(&self) -> &'static str {{ {} }}\n    fn write_cbor(&self, out: &mut ::std::vec::Vec<u8>) -> ::std::result::Result<(), ::std::string::String> {{ ::crucible_server_runtime_rust::__generated::cbor::write_typed(self, self.shape_id(), out) }}\n{status}    fn info(&self) -> &'static ::crucible_server_runtime_rust::__generated::operation::ErrorInfo {{\n        &{}\n    }}\n\n    fn to_json(&self) -> ::crucible_server_runtime_rust::__generated::serde_json::Value {{\n        ::crucible_server_runtime_rust::__generated::serde_json::to_value(self).unwrap_or_default()\n    }}\n\n    fn write_json(&self, out: &mut ::std::vec::Vec<u8>) -> ::crucible_server_runtime_rust::__generated::serde_json::Result<::std::vec::Vec<(&'static str, ::std::string::String)>> {{\n        ::crucible_server_runtime_rust::__generated::error::write_error(self, out)\n    }}\n\n    fn write_http(&self, out: &mut ::std::vec::Vec<u8>) -> ::crucible_server_runtime_rust::__generated::serde_json::Result<::crucible_server_runtime_rust::__generated::operation::ResponseBindings> {{\n        ::crucible_server_runtime_rust::__generated::error::write_error_http(self, out)\n    }}\n\n    fn write_http_typed(&self, type_name: ::std::option::Option<&'static str>, out: &mut ::std::vec::Vec<u8>) -> ::crucible_server_runtime_rust::__generated::serde_json::Result<::crucible_server_runtime_rust::__generated::operation::ResponseBindings> {{\n        ::crucible_server_runtime_rust::__generated::error::write_error_http_typed(self, type_name, out)\n    }}\n}}\n\n",
            literal(&id.to_string()),
            info_static(name)
        ));
        let message = structure
            .member("message")
            .filter(|m| simple_kind(service, &m.target) == Some(SimpleKind::String));
        let display = match message {
            // Handlers and the service log print errors through Display; the
            // response body is written from the fields, never from it.
            Some(m) if structure.sensitive() || service.sensitive(m) => {
                "\"*** Sensitive Data Redacted ***\"".to_owned()
            }
            Some(m)
                if if client {
                    m.optional_on_client()
                } else {
                    m.optional_on_server()
                } =>
            {
                "self.message.as_deref().unwrap_or_default()".to_owned()
            }
            Some(_) => "&self.message".to_owned(),
            None => "\"\"".to_owned(),
        };
        out.push_str(&format!(
            "impl ::std::fmt::Display for {name} {{\n    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        write!(f, \"{{}}: {{}}\", {}, {display})\n    }}\n}}\n\n",
            literal(name)
        ));
        out.push_str(&format!("impl ::std::error::Error for {name} {{}}\n"));
    }
    if !defaults.is_empty() {
        out.push_str("\nmod defaults {\n");
        out.push_str(&defaults);
        out.push_str("}\n");
    }
    out
}

pub(crate) fn info_static(name: &str) -> String {
    format!("{}_INFO", constant(name))
}

/// `fn classify(..)`: the modelled error inside a boxed error, by downcast.
pub(crate) fn classify(service: &Service) -> String {
    let mut out = String::from(
        "fn classify<'a>(error: &'a (dyn ::std::error::Error + 'static)) -> ::std::option::Option<&'a dyn ::crucible_server_runtime_rust::__generated::ModelledError> {\n",
    );
    for (id, _) in error_shapes(service) {
        out.push_str(&format!(
            "    if let Some(error) = error.downcast_ref::<errors::{}>() {{\n        return Some(error);\n    }}\n",
            service.name_of(id)
        ));
    }
    out.push_str("    None\n}\n");
    if error_shapes(service).is_empty() {
        out = out.replace("classify<'a>(error:", "classify<'a>(_error:");
    }
    out
}

/// `fn reject(..)`, when crucible can fill the validation error: `message`
/// takes a summary and a `fieldList` of `path`/`name` + `message` structures
/// takes every violation, up to the list's `@length` maximum. `None` when it
/// needs more (a required `reason`, say): the service gives the builder
/// `on_rejection` instead.
pub(crate) fn reject(service: &Service, validation: Option<&ShapeId>) -> Option<String> {
    let id = validation?;
    let Some(Shape::Structure(structure)) = service.shape(id) else {
        return None;
    };
    let fields = fill(service, structure, |member| match member.name.as_str() {
        "message" if simple_kind(service, &member.target) == Some(SimpleKind::String) => {
            Some("rejection.summary()".to_owned())
        }
        "fieldList" => field_list(service, member),
        "reason" => {
            let malformed = enum_value(service, member, "cannotParse")?;
            let broken = enum_value(service, member, "fieldValidationFailed")?;
            Some(format!(
                "if rejection.is_malformed() {{ {malformed} }} else {{ {broken} }}"
            ))
        }
        _ => None,
    })?;
    Some(format!(
        "fn reject(rejection: &::crucible_server_runtime_rust::__generated::validate::Rejection) -> ::std::boxed::Box<dyn ::crucible_server_runtime_rust::__generated::ModelledError> {{\n    ::std::boxed::Box::new(errors::{} {{\n        {}\n    }})\n}}\n",
        service.name_of(id),
        fields.join(",\n        ")
    ))
}

/// The explicitly selected error, or the shared error if the service reaches it.
/// Do not select arbitrary 404 errors: those can represent missing resources.
pub(crate) fn unknown_operation_error(
    service: &Service,
    configured: Option<&ShapeId>,
) -> Option<ShapeId> {
    if let Some(id) = configured {
        return Some(id.clone());
    }
    let id = ShapeId::new("crucible.errors", "UnknownOperationException");
    matches!(service.shape(&id), Some(Shape::Structure(s)) if s.error.is_some()).then_some(id)
}

/// Fill the modelled routing error through the existing service answer hook.
pub(crate) fn unknown_operation(
    service: &Service,
    unknown: Option<&ShapeId>,
    diagnostics: &mut Diagnostics,
) -> Option<String> {
    let id = unknown?;
    let Some(Shape::Structure(structure)) = service.shape(id) else {
        return None;
    };
    let error = structure.error.as_ref()?;
    if error.fault != Fault::Client || error.status != 404 {
        diagnostics.push(
            id,
            "the unknown-operation error must be a client error with HTTP status 404",
        );
    }
    let fields = fill(service, structure, |member| match member.name.as_str() {
        "message" if simple_kind(service, &member.target) == Some(SimpleKind::String) => {
            Some("\"No operation matches the request's method and path.\".to_owned()".to_owned())
        }
        _ => None,
    })?;
    Some(format!(
        "fn unknown() -> ::std::boxed::Box<dyn ::crucible_server_runtime_rust::__generated::ModelledError> {{\n    ::std::boxed::Box::new(errors::{} {{\n        {}\n    }})\n}}\n",
        service.name_of(id),
        fields.join(",\n        ")
    ))
}

/// `fn unknown()`: the validation error a request no operation serves gets,
/// when the error's `reason` enum has the standard `unknownOperation` value
/// and nothing else required is left unfilled. Without it, the protocol
/// answers its own `UnknownOperation`.
pub(crate) fn unknown(service: &Service, validation: Option<&ShapeId>) -> Option<String> {
    let id = validation?;
    let Some(Shape::Structure(structure)) = service.shape(id) else {
        return None;
    };
    let reason = structure.member("reason")?;
    enum_value(service, reason, "unknownOperation")?;
    let fields = fill(service, structure, |member| match member.name.as_str() {
        "message" if simple_kind(service, &member.target) == Some(SimpleKind::String) => {
            Some("\"No operation matches the request's method and path.\".to_owned()".to_owned())
        }
        "reason" => enum_value(service, member, "unknownOperation"),
        _ => None,
    })?;
    Some(format!(
        "fn unknown() -> ::std::boxed::Box<dyn ::crucible_server_runtime_rust::__generated::ModelledError> {{\n    ::std::boxed::Box::new(errors::{} {{\n        {}\n    }})\n}}\n",
        service.name_of(id),
        fields.join(",\n        ")
    ))
}

/// `model::Enum::Variant` for the value of `member`'s enum target that is
/// the standard value `standard`, if it has one. Spelling is the model's:
/// `cannotParse`, `cannot_parse` and `CANNOT_PARSE` all match, so a service
/// keeps its own enum casing.
fn enum_value(service: &Service, member: &Member, standard: &str) -> Option<String> {
    let Some(Shape::Enum(values)) = service.shape(&member.target) else {
        return None;
    };
    let folded = |text: &str| -> String {
        text.chars()
            .filter(|c| *c != '_')
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    let value = values
        .values
        .iter()
        .find(|value| folded(&value.value) == folded(standard))?;
    Some(format!(
        "model::{}::{}",
        service.name_of(&member.target),
        variant(&value.name)
    ))
}

/// `fn internal()`: the service's internal error with a fixed message, when
/// it has one crucible can fill (a `message`, nothing else required).
pub(crate) fn internal(service: &Service, internal: Option<&ShapeId>) -> Option<String> {
    let id = internal?;
    let Some(Shape::Structure(structure)) = service.shape(id) else {
        return None;
    };
    let fields = fill(service, structure, |member| match member.name.as_str() {
        "message" if simple_kind(service, &member.target) == Some(SimpleKind::String) => Some(
            "\"The request failed. Quote its request id when reporting it.\".to_owned()".to_owned(),
        ),
        _ => None,
    })?;
    Some(format!(
        "fn internal() -> ::std::boxed::Box<dyn ::crucible_server_runtime_rust::__generated::ModelledError> {{\n    ::std::boxed::Box::new(errors::{} {{\n        {}\n    }})\n}}\n",
        service.name_of(id),
        fields.join(",\n        ")
    ))
}

/// Field initializers for every member: `value` fills what it can, optional
/// members it cannot are `None`, and a required one it cannot fails the lot.
/// A string `requestId` member, the way a model carries the request id in
/// every error body, takes the id of the request being answered.
fn fill(
    service: &Service,
    structure: &Structure,
    value: impl Fn(&Member) -> Option<String>,
) -> Option<Vec<String>> {
    const REQUEST_ID: &str = "::crucible_server_runtime_rust::__generated::Context::try_current().map(|context| context.request_id().to_owned())";
    structure
        .members
        .iter()
        .map(|member| {
            let rust = field(&member.name);
            if member.name == "requestId"
                && simple_kind(service, &member.target) == Some(SimpleKind::String)
            {
                return Some(if member.optional_on_server() {
                    format!("{rust}: {REQUEST_ID}")
                } else {
                    format!("{rust}: {REQUEST_ID}.unwrap_or_default()")
                });
            }
            match (value(member), member.optional_on_server()) {
                (Some(value), true) => Some(format!("{rust}: Some({value})")),
                (Some(value), false) => Some(format!("{rust}: {value}")),
                (None, true) => Some(format!("{rust}: None")),
                (None, false) => None,
            }
        })
        .collect()
}

fn field_list(service: &Service, member: &Member) -> Option<String> {
    let Some(Shape::List(list)) = service.shape(&member.target) else {
        return None;
    };
    let Some(Shape::Structure(item)) = service.shape(&list.member.target) else {
        return None;
    };
    let fields = fill(service, item, |field_member| {
        match field_member.name.as_str() {
            "path" | "name" => Some("violation.path.clone()".to_owned()),
            "message" => Some("violation.message.clone()".to_owned()),
            _ => None,
        }
    })?;
    let cap = member
        .constraints
        .length
        .as_ref()
        .and_then(|length| length.max)
        .map(|max| format!(".take({max})"))
        .unwrap_or_default();
    Some(format!(
        "rejection.violations.iter(){cap}.map(|violation| model::{} {{ {} }}).collect()",
        service.name_of(&list.member.target),
        fields.join(", ")
    ))
}

/// A configured error must be an error structure the service reaches; a typo
/// fails generation here rather than surfacing as a runtime build error.
pub(crate) fn configured_error(
    service: &Service,
    configured: Option<&ShapeId>,
    what: &str,
    diagnostics: &mut Diagnostics,
) -> bool {
    let Some(id) = configured else {
        return true;
    };
    let is_error = matches!(service.shape(id), Some(Shape::Structure(s)) if s.error.is_some());
    if !is_error {
        diagnostics.push(
            id,
            format!("the configured {what} is not an @error structure the service reaches"),
        );
    }
    is_error
}

/// The error a rejected request becomes: the configured one, else the
/// service's one error named `*Validation*`.
pub(crate) fn validation_error(service: &Service, configured: Option<&ShapeId>) -> Option<ShapeId> {
    if let Some(id) = configured {
        return Some(id.clone());
    }
    let candidates: Vec<&ShapeId> = error_shapes(service)
        .into_iter()
        .filter(|(id, _)| service.name_of(id).contains("Validation"))
        .map(|(id, _)| id)
        .collect();
    match candidates.as_slice() {
        [only] => Some((*only).clone()),
        _ => None,
    }
}

/// The error internal failures are answered with: the configured one, else
/// the service's one server error with status 500.
pub(crate) fn internal_error(service: &Service, configured: Option<&ShapeId>) -> Option<ShapeId> {
    if let Some(id) = configured {
        return Some(id.clone());
    }
    let candidates: Vec<&ShapeId> = error_shapes(service)
        .into_iter()
        .filter(|(_, s)| {
            s.error
                .as_ref()
                .is_some_and(|e| e.fault == Fault::Server && e.status == 500)
        })
        .map(|(id, _)| id)
        .collect();
    match candidates.as_slice() {
        [only] => Some((*only).clone()),
        _ => None,
    }
}
