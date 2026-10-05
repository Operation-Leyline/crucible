//! The client's top level: per operation its marker type, an error enum of
//! the errors it declares, and a method on `Client`; `*_pages` for a
//! `@paginated` operation.

use crucible_smithy::ir::{Member, Operation, Service, SimpleKind};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::errors::info_static;
use crate::names::{doc, field, function, literal};
use crate::operations::{input_type, marker, output_type, structure};
use crate::types::simple_kind;

const UNIT: &str = "::crucible::operation::Unit";

pub(crate) fn emit(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let mut out = String::new();
    let mut methods = String::new();
    for op in &service.operations {
        let output = if op.output.is_some() {
            output_type(service, op)
        } else {
            UNIT.to_owned()
        };
        out.push_str(&marker(service, op, &output));
        out.push_str(&error_enum(service, op));
        methods.push_str(&method(service, op));
        methods.push_str(&pages(service, op, diagnostics));
        if function(op.name()) == "new" {
            diagnostics.push(&op.id, "an operation named New collides with Client::new");
        }
    }
    out.push_str(&format!(
        "\n/// A client for {}. Cheap to clone: clones share connections.\n#[derive(Clone)]\npub struct Client {{\n    transport: ::crucible::Transport,\n}}\n\nimpl Client {{\n    pub fn new(config: ::crucible::Config) -> Self {{\n        Self {{\n            transport: ::crucible::Transport::new(config),\n        }}\n    }}\n{methods}}}\n",
        service.id.name()
    ));
    out
}

fn error_type(op: &Operation) -> String {
    format!("{}Error", op.name())
}

/// `enum GetCityError`: one variant per declared error, decoded by the
/// body's `__type`, else by status when only one error has it.
fn error_enum(service: &Service, op: &Operation) -> String {
    let name = error_type(op);
    let errors: Vec<&str> = op.errors.iter().map(|id| service.name_of(id)).collect();
    let mut out = format!(
        "\n/// The errors {} declares.\n#[derive(Debug, Clone, PartialEq)]\npub enum {name} {{\n",
        op.name()
    );
    for error in &errors {
        out.push_str(&format!("    {error}(errors::{error}),\n"));
    }
    out.push_str("}\n\n");
    let infos: Vec<String> = errors
        .iter()
        .map(|error| format!("&errors::{}", info_static(error)))
        .collect();
    let arms: String = errors
        .iter()
        .map(|error| {
            format!(
                "            {} => ::crucible::decode_modelled(&errors::{}, headers, body).map(Self::{error}),\n",
                literal(error),
                info_static(error)
            )
        })
        .collect();
    let (info_body, display_body) = if errors.is_empty() {
        (
            "match *self {}".to_owned(),
            "let _ = f;\n        match *self {}".to_owned(),
        )
    } else {
        let info: String = errors
            .iter()
            .map(|error| {
                format!(
                    "            Self::{error}(error) => ::crucible::ModelledError::info(error),\n"
                )
            })
            .collect();
        let display: String = errors
            .iter()
            .map(|error| {
                format!("            Self::{error}(error) => ::std::fmt::Display::fmt(error, f),\n")
            })
            .collect();
        (
            format!("match self {{\n{info}        }}"),
            format!("match self {{\n{display}        }}"),
        )
    };
    let decode_body = if errors.is_empty() {
        "        let _ = (status, type_name, headers, body);\n        None\n".to_owned()
    } else {
        format!(
            "        let name = match type_name {{\n            Some(name) => name,\n            None => ::crucible::by_status(status, &[{}])?,\n        }};\n        match name {{\n{arms}            _ => None,\n        }}\n",
            infos.join(", ")
        )
    };
    out.push_str(&format!(
        "impl ::crucible::OperationErrors for {name} {{\n    fn decode(status: u16, type_name: ::std::option::Option<&str>, headers: &::crucible::__private::http::HeaderMap, body: &[u8]) -> ::std::option::Option<Self> {{\n{decode_body}    }}\n\n    fn info(&self) -> &'static ::crucible::operation::ErrorInfo {{\n        {info_body}\n    }}\n}}\n\n"
    ));
    out.push_str(&format!(
        "impl ::std::fmt::Display for {name} {{\n    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        {display_body}\n    }}\n}}\n\nimpl ::std::error::Error for {name} {{}}\n"
    ));
    out
}

fn method(service: &Service, op: &Operation) -> String {
    let name = op.name();
    let errors = error_type(op);
    let mut out = String::from("\n");
    doc(&mut out, "    ", op.docs.as_deref());
    let output = if op.output.is_some() {
        output_type(service, op)
    } else {
        "()".to_owned()
    };
    let result = format!("::std::result::Result<{output}, ::crucible::Error<{errors}>>");
    let send = format!("self.transport.send::<{name}, {errors}>");
    let finish = if op.output.is_some() {
        ".await"
    } else {
        ".await.map(|_| ())"
    };
    if op.input.is_none() {
        out.push_str(&format!(
            "    pub async fn {}(&self) -> {result} {{\n        {send}(&{UNIT} {{}}){finish}\n    }}\n",
            function(name)
        ));
        return out;
    }
    let token = token_member(service, op).map(|member| {
        let rust = field(&member.name);
        format!(
            "        if input.{rust}.value().is_none() {{\n            input.{rust} = ::crucible::Field::Value(::crucible::idempotency_token());\n        }}\n"
        )
    });
    let binding = if token.is_some() {
        "mut input"
    } else {
        "input"
    };
    out.push_str(&format!(
        "    pub async fn {}(&self, {binding}: {}) -> {result} {{\n{}        {send}(&input){finish}\n    }}\n",
        function(name),
        input_type(service, op),
        token.unwrap_or_default(),
    ));
    out
}

/// The idempotency token member, when the client may fill it: an optional
/// string.
fn token_member<'s>(service: &'s Service, op: &Operation) -> Option<&'s Member> {
    let name = op.idempotency_token.as_ref()?;
    let member = structure(service, op.input.as_ref())?.member(name)?;
    let string = simple_kind(service, &member.target) == Some(SimpleKind::String);
    (string && member.optional_on_client()).then_some(member)
}

/// `*_pages(input)`: a `Paginator` that resends the input with each
/// response's token. Tokens must be top-level string members.
fn pages(service: &Service, op: &Operation, diagnostics: &mut Diagnostics) -> String {
    let Some(paginated) = &op.paginated else {
        return String::new();
    };
    let input = string_member(service, op.input.as_ref(), &paginated.input_token);
    let output = string_member(service, op.output.as_ref(), &paginated.output_token);
    let (Some(input), Some(output)) = (input, output) else {
        diagnostics.push(
            &op.id,
            "@paginated tokens must be top-level string members of the input and output",
        );
        return String::new();
    };
    let get = |member: &Member, value: &str| {
        let rust = field(&member.name);
        if member.optional_on_client() {
            format!("|{value}| {value}.{rust}.as_deref()")
        } else {
            format!("|{value}| Some(&{value}.{rust})")
        }
    };
    let set = if input.optional_on_client() {
        format!(
            "|input, token| input.{} = ::crucible::Field::Value(token)",
            field(&input.name)
        )
    } else {
        format!("|input, token| input.{} = token", field(&input.name))
    };
    let name = op.name();
    let errors = error_type(op);
    // A token the caller left empty is filled per page, so every page is
    // retried under its own key; a caller's own token is kept.
    let (start, finish) = match token_member(service, op) {
        Some(member) if member.name != input.name => {
            let rust = field(&member.name);
            (
                format!("let fresh = input.{rust}.value().is_none();\n        let pages = "),
                format!(
                    ";\n        if fresh {{\n            pages.fresh_token(|input| input.{rust} = ::crucible::Field::Value(::crucible::idempotency_token()))\n        }} else {{\n            pages\n        }}"
                ),
            )
        }
        _ => (String::new(), String::new()),
    };
    format!(
        "\n    /// Every page of {name}, one request each.\n    pub fn {}_pages(&self, input: {}) -> ::crucible::Paginator<{name}, {errors}> {{\n        {start}::crucible::Paginator::<{name}, {errors}>::new(\n            self.transport.clone(),\n            input,\n            {},\n            {},\n            {set},\n        ){finish}\n    }}\n",
        function(name).trim_start_matches("r#"),
        input_type(service, op),
        get(output, "output"),
        get(input, "input"),
    )
}

fn string_member<'s>(service: &'s Service, id: Option<&ShapeId>, name: &str) -> Option<&'s Member> {
    let member = structure(service, id)?.member(name)?;
    (simple_kind(service, &member.target) == Some(SimpleKind::String)).then_some(member)
}
