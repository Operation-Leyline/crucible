//! `router.rs`: the axum router, and for each operation a handler that reads
//! its input from the request, validates it, calls the service and writes
//! the response.

use std::collections::BTreeMap;

use crucible_core::ir::{
    Binding, Member, Operation, Segment, Service, Shape, SimpleKind, Structure, TimestampFormat,
};
use crucible_core::{Diagnostics, ShapeId};

use crate::api::traits;
use crate::errors::header_push;
use crate::models::member_field;
use crate::names::{field, function, literal};
use crate::types::{DATE_TIME, boxed_members, builtin};

pub(crate) fn emit(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let boxed = boxed_members(service);
    let mut out = String::from(crate::HEADER);
    out.push_str("#![allow(clippy::all)]\n\n");
    out.push_str("use crate::runtime::{FromText, Parts, Rejection, ToText, Validate, Validator, read, take};\n");

    let mut bounds: Vec<String> = traits(service)
        .keys()
        .map(|name| format!("crate::api::{name}"))
        .collect();
    bounds.push("crate::api::Reject".to_owned());
    out.push_str(&format!(
        "\n/// Every operation of the service, routed to `service`.\npub fn router<S>(service: S) -> ::axum::Router\nwhere\n    S: {} + Clone + Send + Sync + 'static,\n{{\n    ::axum::Router::new()\n",
        bounds.join(" + ")
    ));
    for (path, operations) in routes(service, diagnostics) {
        let methods: Vec<String> = operations
            .iter()
            .map(|op| {
                format!(
                    "{}({}::<S>)",
                    method_router(op, diagnostics),
                    function(op.name())
                )
            })
            .collect();
        let first = methods[0].clone();
        let chained = methods[1..]
            .iter()
            .fold(format!("::axum::routing::{first}"), |acc, next| {
                format!("{acc}.{next}")
            });
        out.push_str(&format!("        .route({}, {chained})\n", literal(&path)));
    }
    out.push_str("        .with_state(service)\n}\n");

    let mut defaults = String::new();
    for operation in &service.operations {
        out.push_str(&handler(service, operation));
        out.push_str(&reader(
            service,
            operation,
            &boxed,
            &mut defaults,
            diagnostics,
        ));
        out.push_str(&writer(service, operation));
    }
    if !defaults.is_empty() {
        out.push_str("\nmod defaults {\n");
        out.push_str(&defaults);
        out.push_str("}\n");
    }
    out
}

/// Operations by path, in path order. Two paths that differ only in label
/// names would make axum panic at startup, so they are a diagnostic here.
fn routes<'s>(
    service: &'s Service,
    diagnostics: &mut Diagnostics,
) -> BTreeMap<String, Vec<&'s Operation>> {
    let mut routes: BTreeMap<String, Vec<&Operation>> = BTreeMap::new();
    let mut shapes: BTreeMap<String, String> = BTreeMap::new();
    for operation in &service.operations {
        if !operation.uri.query.is_empty() {
            diagnostics.push(
                &operation.id,
                "a literal query string in the URI is not routed yet",
            );
        }
        let path = path(operation);
        let shape: String = operation
            .uri
            .segments
            .iter()
            .map(|s| match s {
                Segment::Literal(text) => format!("/{text}"),
                Segment::Label(_) => "/{}".to_owned(),
            })
            .collect();
        match shapes.get(&shape) {
            Some(existing) if *existing != path => diagnostics.push(
                &operation.id,
                format!("{path} and {existing} differ only in label names, which one router cannot serve"),
            ),
            _ => {
                shapes.insert(shape, path.clone());
            }
        }
        let same = routes.entry(path.clone()).or_default();
        if same.iter().any(|other| other.method == operation.method) {
            diagnostics.push(
                &operation.id,
                format!(
                    "another operation already serves {} {path}",
                    operation.method
                ),
            );
        }
        same.push(operation);
    }
    routes
}

fn path(operation: &Operation) -> String {
    if operation.uri.segments.is_empty() {
        return "/".to_owned();
    }
    operation
        .uri
        .segments
        .iter()
        .map(|s| match s {
            Segment::Literal(text) => format!("/{text}"),
            Segment::Label(label) => format!("/{{{label}}}"),
        })
        .collect()
}

fn method_router(operation: &Operation, diagnostics: &mut Diagnostics) -> &'static str {
    match operation.method.as_str() {
        "GET" => "get",
        "POST" => "post",
        "PUT" => "put",
        "DELETE" => "delete",
        "PATCH" => "patch",
        "HEAD" => "head",
        "OPTIONS" => "options",
        other => {
            diagnostics.push(&operation.id, format!("HTTP method {other} is not routed"));
            "get"
        }
    }
}

fn handler(service: &Service, operation: &Operation) -> String {
    let name = function(operation.name());
    let api = crate::api::traits(service)
        .into_iter()
        .find(|(_, ops)| ops.iter().any(|op| op.id == operation.id))
        .map(|(name, _)| name)
        .expect("every operation has a trait");
    let labels = !operation.request.labels.is_empty();
    let label_param = if labels {
        "    labels: ::axum::extract::RawPathParams,\n"
    } else {
        ""
    };
    let label_arg = if labels { "Some(&labels)" } else { "None" };
    let call = match &operation.input {
        Some(_) => format!(
            "    let parts = Parts::new({label_arg}, query.as_deref(), &headers);\n    let input = match read_{name}(&parts, &body) {{\n        Ok(input) => input,\n        Err(rejection) => return crate::runtime::error_response(&service.reject(rejection)),\n    }};\n    let ctx = crate::runtime::Context {{ method, uri, headers, extensions }};\n    match service.{name}(&ctx, input).await {{\n"
        ),
        None => "    let _ = (query, body);\n    let ctx = crate::runtime::Context { method, uri, headers, extensions };\n".to_owned()
            + &format!("    match service.{name}(&ctx).await {{\n"),
    };
    let ok = if operation.output.is_some() {
        "Ok(output) => write_".to_owned() + &name + "(&output)"
    } else {
        format!("Ok(()) => write_{name}()")
    };
    format!(
        "\nasync fn {name}<S: crate::api::{api} + crate::api::Reject>(\n    ::axum::extract::State(service): ::axum::extract::State<S>,\n{label_param}    ::axum::extract::RawQuery(query): ::axum::extract::RawQuery,\n    method: ::axum::http::Method,\n    uri: ::axum::http::Uri,\n    headers: ::axum::http::HeaderMap,\n    extensions: ::axum::http::Extensions,\n    body: ::axum::body::Bytes,\n) -> ::axum::response::Response {{\n{call}        {ok},\n        Err(error) => error.into_response(),\n    }}\n}}\n"
    )
}

fn input_structure<'s>(
    service: &'s Service,
    operation: &Operation,
) -> Option<(&'s ShapeId, &'s Structure)> {
    let id = operation.input.as_ref()?;
    match service.shape(id)? {
        Shape::Structure(structure) => Some((service.shapes.get_key_value(id)?.0, structure)),
        _ => None,
    }
}

fn reader(
    service: &Service,
    operation: &Operation,
    boxed: &std::collections::BTreeSet<ShapeId>,
    defaults: &mut String,
    diagnostics: &mut Diagnostics,
) -> String {
    let Some((input_id, input)) = input_structure(service, operation) else {
        return String::new();
    };
    let name = function(operation.name());
    let body_name = format!("{}RequestBody", operation.name());
    let mut out = String::new();
    let has_body = !operation.request.body.is_empty();
    if has_body {
        out.push_str(&format!(
            "\n#[derive(::serde::Deserialize)]\nstruct {body_name} {{\n"
        ));
        for member_name in &operation.request.body {
            let member = input
                .member(member_name)
                .expect("bindings name input members");
            let is_boxed = boxed.contains(&input_id.with_member(member_name));
            member_field(
                &mut out,
                service,
                input_id,
                member,
                is_boxed,
                "",
                "crate::models::",
                defaults,
                diagnostics,
            );
        }
        out.push_str("}\n");
    }

    out.push_str(&format!(
        "\nfn read_{name}(parts: &Parts, body: &::axum::body::Bytes) -> ::std::result::Result<crate::models::{}, Rejection> {{\n",
        service.name_of(input_id)
    ));
    let mut taken: Vec<String> = Vec::new();
    let mut body_lines = String::new();
    if has_body {
        body_lines.push_str(&format!(
            "    let body = take(&mut rejection, crate::runtime::json_body::<{body_name}>(body));\n"
        ));
        taken.push("body".to_owned());
    } else {
        body_lines.push_str("    let _ = body;\n");
    }
    let bound = |bindings: &[Binding], kind: Bound| {
        bindings
            .iter()
            .map(move |b| (b.member.clone(), b.name.clone(), kind))
            .collect::<Vec<_>>()
    };
    let mut bound_members = Vec::new();
    for label in &operation.request.labels {
        bound_members.push((label.clone(), label.clone(), Bound::Label));
    }
    bound_members.extend(bound(&operation.request.query, Bound::Query));
    bound_members.extend(bound(&operation.request.headers, Bound::Header));
    for (member_name, wire, kind) in &bound_members {
        let member = input
            .member(member_name)
            .expect("bindings name input members");
        if member.default.is_some() && !member.required {
            diagnostics.push(
                &input_id.with_member(member_name),
                "a default on a bound member is not generated yet",
            );
        }
        let variable = format!("m_{}", field(member_name).trim_start_matches("r#"));
        body_lines.push_str(&format!(
            "    let {variable} = take(&mut rejection, {});\n",
            bound_expr(service, member, wire, *kind)
        ));
        taken.push(variable);
    }
    if taken.is_empty() {
        out.push_str("    let _ = parts;\n");
        out.push_str(&body_lines);
    } else {
        out.push_str("    let mut rejection = Rejection { violations: ::std::vec::Vec::new() };\n");
        out.push_str(&body_lines);
        if bound_members.is_empty() {
            out.push_str("    let _ = parts;\n");
        }
        let somes: Vec<String> = taken.iter().map(|t| format!("Some({t})")).collect();
        out.push_str(&format!(
            "    let ({},) = ({},) else {{\n        return Err(rejection);\n    }};\n",
            somes.join(", "),
            taken.join(", ")
        ));
    }
    let fields: Vec<String> = input
        .members
        .iter()
        .map(|member| {
            let rust = field(&member.name);
            if operation.request.body.contains(&member.name) {
                format!("{rust}: body.{rust}")
            } else {
                format!("{rust}: m_{}", rust.trim_start_matches("r#"))
            }
        })
        .collect();
    let fields = if fields.is_empty() {
        " {}".to_owned()
    } else {
        format!(" {{\n        {},\n    }}", fields.join(",\n        "))
    };
    out.push_str(&format!(
        "    let input = crate::models::{}{fields};\n    let mut validator = Validator::default();\n    input.validate(&mut validator);\n    validator.finish()?;\n    Ok(input)\n}}\n",
        service.name_of(input_id),
    ));
    out
}

#[derive(Debug, Clone, Copy)]
enum Bound {
    Label,
    Query,
    Header,
}

/// A `Result<FieldType, Rejection>` expression reading a bound member.
fn bound_expr(service: &Service, member: &Member, wire: &str, kind: Bound) -> String {
    let wire_literal = literal(wire);
    let list = match service.shape(&member.target) {
        Some(Shape::List(list)) => Some(&list.member),
        _ => None,
    };
    let parser = parser(service, list.unwrap_or(member));
    let required = !member.optional_on_server();
    let require = |expr: String| {
        if required {
            format!(
                "{expr}.and_then(|value| value.ok_or_else(|| Rejection::one({wire_literal}, \"is required\")))"
            )
        } else {
            expr
        }
    };
    match (kind, list) {
        (Bound::Label, _) => format!(
            "parts.label({wire_literal}).and_then(|text| read({wire_literal}, text, {parser}))"
        ),
        (Bound::Query, None) => require(format!(
            "parts.query({wire_literal}).map(|text| read({wire_literal}, text, {parser})).transpose()"
        )),
        (Bound::Query, Some(_)) => {
            let items = format!(
                "parts.query_all({wire_literal}).into_iter().map(|text| read({wire_literal}, text, {parser})).collect::<::std::result::Result<::std::vec::Vec<_>, _>>()"
            );
            if required {
                items
            } else {
                format!("{items}.map(|items| (!items.is_empty()).then_some(items))")
            }
        }
        (Bound::Header, None) => require(format!(
            "parts.header({wire_literal}).and_then(|text| text.map(|text| read({wire_literal}, text, {parser})).transpose())"
        )),
        (Bound::Header, Some(_)) => require(format!(
            "parts.header({wire_literal}).and_then(|text| text.map(|text| crate::runtime::split_header(text).iter().map(|item| read({wire_literal}, item, {parser})).collect::<::std::result::Result<::std::vec::Vec<_>, _>>()).transpose())"
        )),
    }
}

/// A function from text to the member's (scalar) type.
fn parser(service: &Service, member: &Member) -> String {
    match service.shape(&member.target) {
        Some(Shape::Enum(_)) => format!(
            "<crate::models::{} as FromText>::from_text",
            service.name_of(&member.target)
        ),
        Some(Shape::Simple(simple)) if simple.kind == SimpleKind::Timestamp => {
            match service.timestamp_format(member) {
                TimestampFormat::DateTime => format!("<{DATE_TIME} as FromText>::from_text"),
                TimestampFormat::EpochSeconds => "crate::runtime::parse_epoch_seconds".to_owned(),
                TimestampFormat::HttpDate => "crate::runtime::parse_http_date".to_owned(),
            }
        }
        Some(Shape::Simple(simple)) => format!("<{} as FromText>::from_text", builtin(simple.kind)),
        _ => "<::std::string::String as FromText>::from_text".to_owned(),
    }
}

fn writer(service: &Service, operation: &Operation) -> String {
    let name = function(operation.name());
    let status = operation.status;
    let Some(output_id) = &operation.output else {
        return format!(
            "\nfn write_{name}() -> ::axum::response::Response {{\n    crate::runtime::ok_response::<()>({status}, ::std::vec::Vec::new(), None)\n}}\n"
        );
    };
    let Some(Shape::Structure(output)) = service.shape(output_id) else {
        return String::new();
    };
    let mut out = format!(
        "\nfn write_{name}(output: &crate::models::{}) -> ::axum::response::Response {{\n",
        service.name_of(output_id)
    );
    if operation.response.headers.is_empty() {
        out.push_str("    let headers = ::std::vec::Vec::new();\n");
    } else {
        out.push_str("    let mut headers = ::std::vec::Vec::new();\n");
        for binding in &operation.response.headers {
            if let Some(member) = output.member(&binding.member) {
                out.push_str(&header_push(
                    service,
                    member,
                    "output",
                    &binding.name,
                    "    ",
                ));
            }
        }
    }
    let body = if operation.response.body.is_empty() {
        "None::<&()>"
    } else {
        "Some(output)"
    };
    out.push_str(&format!(
        "    crate::runtime::ok_response({status}, headers, {body})\n}}\n"
    ));
    out
}
