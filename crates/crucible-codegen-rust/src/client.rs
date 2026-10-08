//! The client's top level: per operation its marker type, an error enum of
//! the errors it declares, and a method on the service client; `*_pages` for a
//! `@paginated` operation.

use crucible_smithy::ir::{Member, Operation, Service, SimpleKind};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::errors::info_static;
use crate::names::{doc, field, function, literal, variant};
use crate::operations::{input_type, marker, output_type, structure};
use crate::types::simple_kind;

const UNIT: &str = "::crucible_server_runtime_rust::operation::Unit";

pub(crate) fn emit(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let client = format!("{}Client", variant(service.id.name()));
    for id in service.shapes.keys() {
        if !id.is_prelude() && service.name_of(id) == client {
            diagnostics.push(
                id,
                format!("model name collides with generated client `{client}`"),
            );
        }
    }
    let mut out = service_exception(service, diagnostics);
    if service
        .operations
        .iter()
        .any(|op| metadata_needed(service, op))
    {
        let mut schema = crucible_codegen_common::schema(service, str::to_owned);
        schema["operations"]
            .as_object_mut()
            .expect("schema operation object")
            .retain(|name, _| {
                service
                    .operations
                    .iter()
                    .any(|op| op.name() == name && metadata_needed(service, op))
            });
        out.push_str(&format!("\nstatic __CRUCIBLE_CLIENT_METADATA: ::std::sync::LazyLock<::crucible_server_runtime_rust::ServiceMetadata> = ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::ServiceMetadata::from_json({}).expect(\"compiler validated client metadata\"));\n", literal(&schema.to_string())));
    }
    out.push_str(&crate::events::statics(service, diagnostics, true));
    let mut methods = String::new();
    let mut method_names = method_names(service).into_iter();
    for op in &service.operations {
        let output = if op.output.is_some() {
            output_type(service, op).replace("model::", "")
        } else {
            UNIT.to_owned()
        };
        out.push_str(&marker(service, op, &output, true));
        out.push_str(&error_enum(service, op));
        if metadata_needed(service, op) {
            out.push_str(&format!("\n#[allow(non_upper_case_globals)]\nstatic __crucible_metadata_{}: ::std::sync::LazyLock<&'static ::crucible_server_runtime_rust::OperationMetadata> = ::std::sync::LazyLock::new(|| __CRUCIBLE_CLIENT_METADATA.operation({}));\n",op.name(),literal(op.name())));
        }
        methods.push_str(&method(service, op, None));
        let derived = method_names.next().expect("a method set per operation");
        methods.push_str(&method(service, op, Some(&derived.options)));
        methods.push_str(&pages(service, op, derived.pages.as_deref(), diagnostics));
        for (waiter, name) in op.waiters.keys().zip(&derived.waiters) {
            methods.push_str(&waiter_method(service, op, waiter, name));
        }
        if function(op.name()) == "new" {
            diagnostics.push(
                &op.id,
                format!("an operation named New collides with {client}::new"),
            );
        }
        if op.name() == client || error_type(op) == client {
            diagnostics.push(
                &op.id,
                format!("operation declaration collides with generated client `{client}`"),
            );
        }
    }
    out.push_str(&format!(
        "\n/// A client for {}. Cheap to clone: clones share connections.\n#[derive(Clone)]\npub struct {client} {{\n    transport: ::crucible_server_runtime_rust::Transport,\n}}\n\n#[allow(dead_code)]\nimpl {client} {{\n    pub fn new(config: ::crucible_server_runtime_rust::Config) -> Self {{\n        Self {{\n            transport: ::crucible_server_runtime_rust::Transport::new(config),\n        }}\n    }}\n{methods}}}\n",
        service.id.name()
    ));
    out
}

/// The generated methods an operation adds beside its own.
struct Derived {
    options: String,
    /// Only a `@paginated` operation has pages.
    pages: Option<String>,
    /// One per waiter, in the operation's waiter order.
    waiters: Vec<String>,
}

/// Method names for every operation, in operation order. An operation's own
/// method always keeps its name; a derived method (`*_with_options`, `*_pages`,
/// `wait_until_*`) takes a numeric suffix instead of colliding with it or
/// with another derived method, so `ListThings` beside `ListThingsPages`
/// stays valid.
fn method_names(service: &Service) -> Vec<Derived> {
    let mut taken: std::collections::BTreeSet<String> = service
        .operations
        .iter()
        .map(|op| function(op.name()))
        .collect();
    let mut unique = |base: String| {
        let mut name = base.clone();
        let mut suffix = 2;
        while !taken.insert(name.clone()) {
            name = format!("{base}_{suffix}");
            suffix += 1;
        }
        name
    };
    service
        .operations
        .iter()
        .map(|op| {
            let base = function(op.name());
            let base = base.trim_start_matches("r#");
            Derived {
                options: unique(format!("{base}_with_options")),
                pages: op
                    .paginated
                    .as_ref()
                    .map(|_| unique(format!("{base}_pages"))),
                waiters: op
                    .waiters
                    .keys()
                    .map(|waiter| {
                        unique(format!(
                            "wait_until_{}",
                            function(waiter).trim_start_matches("r#")
                        ))
                    })
                    .collect(),
            }
        })
        .collect()
}

/// The client method that pages through `op`, or `None` without `@paginated`.
/// Exposed so the generated service CLI calls the method the SDK names.
pub fn pages_method(service: &Service, op: &Operation) -> Option<String> {
    let index = service.operations.iter().position(|o| o.id == op.id)?;
    method_names(service).swap_remove(index).pages
}

/// One native enum lets callers unify modeled failures from different operations.
fn service_exception(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let name = variant(&crucible_codegen_common::service_exception_name(service));
    for id in service.shapes.keys().filter(|id| !id.is_prelude()) {
        if service.name_of(id) == name {
            diagnostics.push(
                id,
                format!("model name collides with generated service exception `{name}`"),
            );
        }
    }
    for op in &service.operations {
        if op.name() == name || error_type(op) == name {
            diagnostics.push(
                &op.id,
                format!("operation declaration collides with generated service exception `{name}`"),
            );
        }
    }
    let errors = crate::errors::error_shapes(service);
    let mut out = format!(
        "\n/// All modeled errors from {}. Unknown and transport failures remain in `crucible_client_runtime_rust::Error`.\n#[allow(dead_code)]\n#[derive(Debug, Clone, PartialEq)]\n#[non_exhaustive]\npub enum {name} {{\n",
        service.id.name()
    );
    for (id, _) in &errors {
        let error = service.name_of(id);
        out.push_str(&format!("    {error}(errors::{error}),\n"));
    }
    out.push_str("}\n");
    for (id, _) in &errors {
        let error = service.name_of(id);
        out.push_str(&format!("impl From<errors::{error}> for {name} {{\n    fn from(error: errors::{error}) -> Self {{ Self::{error}(error) }}\n}}\n"));
    }
    let display = if errors.is_empty() {
        "let _ = f; match *self {}".to_owned()
    } else {
        let arms: String = errors
            .iter()
            .map(|(id, _)| {
                let error = service.name_of(id);
                format!("Self::{error}(error) => ::std::fmt::Display::fmt(error, f),")
            })
            .collect();
        format!("match self {{ {arms} }}")
    };
    out.push_str(&format!("impl ::std::fmt::Display for {name} {{\n    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{ {display} }}\n}}\nimpl ::std::error::Error for {name} {{}}\n"));
    for op in &service.operations {
        let operation = error_type(op);
        let arms: String = op
            .errors
            .iter()
            .map(|id| {
                let error = service.name_of(id);
                format!("            {operation}::{error}(error) => Self::{error}(error),\n")
            })
            .collect();
        out.push_str(&format!("impl From<{operation}> for {name} {{\n    fn from(error: {operation}) -> Self {{\n        match error {{\n{arms}        }}\n    }}\n}}\n"));
    }
    out
}

fn metadata_needed(service: &Service, op: &Operation) -> bool {
    service.endpoint_rules.is_some() || op.endpoint.is_some() || !op.waiters.is_empty()
}

fn waiter_method(service: &Service, op: &Operation, waiter: &str, method: &str) -> String {
    let name = op.name();
    let errors = error_type(op);
    let output = if op.output.is_some() {
        output_type(service, op).replace("model::", "")
    } else {
        UNIT.to_owned()
    };
    let (parameter, input, token) = if op.input.is_some() {
        let token = token_member(service,op).map(|member|format!("        if input.{}.value().is_none() {{ input.{} = ::crucible_server_runtime_rust::Field::Value(::crucible_server_runtime_rust::idempotency_token()); }}\n",field(&member.name),field(&member.name))).unwrap_or_default();
        (
            format!(
                "{}input: {}, ",
                if token.is_empty() { "" } else { "mut " },
                input_type(service, op).replace("model::", "")
            ),
            "input".to_owned(),
            token,
        )
    } else {
        (String::new(), format!("{UNIT} {{}}"), String::new())
    };
    format!(
        "\n    pub async fn {method}(&self, {parameter}options: ::crucible_server_runtime_rust::WaiterOptions) -> ::std::result::Result<::crucible_server_runtime_rust::WaiterResult<{output}, {errors}>, ::crucible_server_runtime_rust::WaiterError<{output}, {errors}>> {{\n{token}        self.transport.wait::<{name}, {errors}>(&{input}, &__crucible_metadata_{name}, {}, &options).await\n    }}\n",
        literal(waiter)
    )
}

pub(crate) fn error_type(op: &Operation) -> String {
    format!("{}Error", op.name())
}

fn wire_error(service: &Service, op: &Operation, name: &str) -> String {
    let id = op
        .errors
        .iter()
        .find(|id| service.name_of(id) == name)
        .expect("declared operation error");
    let crucible_smithy::ir::Shape::Structure(shape) = service.shape(id).expect("error shape")
    else {
        unreachable!("error structure")
    };
    if service.protocol.is_rpc_v2() {
        return id.to_string();
    }
    shape
        .error
        .as_ref()
        .expect("modeled error metadata")
        .wire_code
        .clone()
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
            if service.protocol == crucible_smithy::Protocol::SmithyRpcV2Cbor {
                format!("            {} => ::crucible_server_runtime_rust::cbor::read(body).ok().map(Self::{error}),\n", literal(&wire_error(service, op, error)))
            } else { format!(
                "            {} => ::crucible_server_runtime_rust::decode_modelled_at(status, &errors::{}, headers, body).map(Self::{error}),\n",
                literal(&wire_error(service,op,error)),
                info_static(error)
            ) }
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
                    "            Self::{error}(error) => ::crucible_server_runtime_rust::ModelledError::info(error),\n"
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
    let fallback = if service.protocol.is_rpc_v2() {
        "type_name?".to_owned()
    } else {
        format!(
            "type_name.or_else(|| ::crucible_server_runtime_rust::by_status(status, &[{}]))?",
            infos.join(", ")
        )
    };
    let decode_body = if errors.is_empty() {
        "        let _ = (status, type_name, headers, body);\n        None\n".to_owned()
    } else if service.protocol.is_rpc_v2() {
        format!(
            "        let _ = (status, headers);\n        let name = {fallback};\n        match name {{\n{arms}            _ => None,\n        }}\n"
        )
    } else {
        format!(
            "        let name = match type_name {{\n            Some(name) => name,\n            None => ::crucible_server_runtime_rust::by_status(status, &[{}])?,\n        }};\n        match name {{\n{arms}            _ => None,\n        }}\n",
            infos.join(", ")
        )
    };
    let classify = if errors.is_empty() {
        "let _ = (status, type_name); None".to_owned()
    } else {
        let arms = errors
            .iter()
            .map(|error| {
                format!(
                    "{} => Some(&errors::{}),",
                    literal(&wire_error(service, op, error)),
                    info_static(error)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        if service.protocol.is_rpc_v2() {
            format!("let _ = status; let name = {fallback}; match name {{ {arms} _ => None }}")
        } else {
            format!("let name = {fallback}; match name {{ {arms} _ => None }}")
        }
    };
    out.push_str(&format!(
        "impl ::crucible_server_runtime_rust::OperationErrors for {name} {{\n    fn decode(status: u16, type_name: ::std::option::Option<&str>, headers: &::crucible_server_runtime_rust::__private::http::HeaderMap, body: &[u8]) -> ::std::option::Option<Self> {{\n{decode_body}    }}\n\n    fn info(&self) -> &'static ::crucible_server_runtime_rust::operation::ErrorInfo {{\n        {info_body}\n    }}\n\n    fn classify(status: u16, type_name: ::std::option::Option<&str>) -> ::std::option::Option<&'static ::crucible_server_runtime_rust::operation::ErrorInfo> {{\n        {classify}\n    }}\n}}\n\n"
    ));
    out.push_str(&format!(
        "impl ::std::fmt::Display for {name} {{\n    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        {display_body}\n    }}\n}}\n\nimpl ::std::error::Error for {name} {{}}\n"
    ));
    out
}

fn method(service: &Service, op: &Operation, options_name: Option<&str>) -> String {
    let name = op.name();
    let errors = error_type(op);
    let mut out = String::from("\n");
    doc(&mut out, "    ", op.docs.as_deref());
    let output = if op.output.is_some() {
        output_type(service, op).replace("model::", "")
    } else {
        "()".to_owned()
    };
    let result =
        format!("::std::result::Result<{output}, ::crucible_server_runtime_rust::Error<{errors}>>");
    let method_name = options_name
        .map(str::to_owned)
        .unwrap_or_else(|| function(name));
    let transport_method = if metadata_needed(service, op) {
        "send_with_metadata"
    } else if options_name.is_some() {
        "send_with_options"
    } else {
        "send"
    };
    let options_parameter = if options_name.is_some() {
        ", options: ::crucible_server_runtime_rust::CallOptions"
    } else {
        ""
    };
    let options_argument = if metadata_needed(service, op) {
        format!(
            ", {}, &__crucible_metadata_{name}",
            if options_name.is_some() {
                "&options"
            } else {
                "&::crucible_server_runtime_rust::CallOptions::default()"
            }
        )
    } else if options_name.is_some() {
        ", &options".to_owned()
    } else {
        String::new()
    };
    let send = format!("self.transport.{transport_method}::<{name}, {errors}>");
    let finish = if op.output.is_some() {
        ".await"
    } else {
        ".await.map(|_| ())"
    };
    if op.input.is_none() {
        out.push_str(&format!(
            "    pub async fn {}(&self{options_parameter}) -> {result} {{\n        {send}(&{UNIT} {{}}{options_argument}){finish}\n    }}\n",
            method_name
        ));
        return out;
    }
    let token = token_member(service, op).map(|member| {
        let rust = field(&member.name);
        format!(
            "        if input.{rust}.value().is_none() {{\n            input.{rust} = ::crucible_server_runtime_rust::Field::Value(::crucible_server_runtime_rust::idempotency_token());\n        }}\n"
        )
    });
    let binding = if token.is_some() {
        "mut input"
    } else {
        "input"
    };
    out.push_str(&format!(
        "    pub async fn {}(&self, {binding}: {}{options_parameter}) -> {result} {{\n{}        {send}(&input{options_argument}){finish}\n    }}\n",
        method_name,
        input_type(service, op).replace("model::", ""),
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
/// response's token, including a nested output token path.
fn pages(
    service: &Service,
    op: &Operation,
    method: Option<&str>,
    diagnostics: &mut Diagnostics,
) -> String {
    let (Some(paginated), Some(method)) = (&op.paginated, method) else {
        return String::new();
    };
    let input = string_member(service, op.input.as_ref(), &paginated.input_token);
    let output = output_token(service, op.output.as_ref(), &paginated.output_token);
    let (Some(input), Some(output)) = (input, output) else {
        diagnostics.push(
            &op.id,
            "@paginated requires a string input token and a string output token path",
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
            "|input, token| input.{} = ::crucible_server_runtime_rust::Field::Value(token)",
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
                    ";\n        if fresh {{\n            pages.fresh_token(|input| input.{rust} = ::crucible_server_runtime_rust::Field::Value(::crucible_server_runtime_rust::idempotency_token()))\n        }} else {{\n            pages\n        }}"
                ),
            )
        }
        _ => (String::new(), String::new()),
    };
    let metadata = if metadata_needed(service, op) {
        format!(".metadata(&__crucible_metadata_{name})")
    } else {
        String::new()
    };
    format!(
        "\n    /// Every page of {name}, one request each.\n    pub fn {method}(&self, input: {}) -> ::crucible_server_runtime_rust::Paginator<{name}, {errors}> {{\n        {start}::crucible_server_runtime_rust::Paginator::<{name}, {errors}>::new(\n            self.transport.clone(),\n            input,\n            {},\n            {},\n            {set},\n        ){metadata}{finish}\n    }}\n",
        input_type(service, op).replace("model::", ""),
        output,
        get(input, "input"),
    )
}

fn string_member<'s>(service: &'s Service, id: Option<&ShapeId>, name: &str) -> Option<&'s Member> {
    let member = structure(service, id)?.member(name)?;
    (simple_kind(service, &member.target) == Some(SimpleKind::String)).then_some(member)
}

fn output_token(service: &Service, id: Option<&ShapeId>, path: &str) -> Option<String> {
    let mut id = id?;
    let mut statements = String::from("|output| { let value = output; ");
    let mut parts = path.split('.').peekable();
    while let Some(name) = parts.next() {
        let member = structure(service, Some(id))?.member(name)?;
        let rust = field(&member.name);
        if member.optional_on_client() {
            statements.push_str(&format!("let value = value.{rust}.value()?; "));
        } else {
            statements.push_str(&format!("let value = &value.{rust}; "));
        }
        if parts.peek().is_none() {
            if simple_kind(service, &member.target) != Some(SimpleKind::String) {
                return None;
            }
            statements.push_str("Some(value.as_str()) }");
        }
        id = &member.target;
    }
    Some(statements)
}
