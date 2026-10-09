//! Rules that hold of every lowered service, whichever front end built it.
//!
//! Each front end parses its own syntax and reports problems in its own
//! terms; what remains true of the IR itself (routes are unique, bindings
//! carry text, pagination members have the right types, examples fit their
//! shapes, names do not collide) is checked here, once, so the Smithy and
//! `.cru` paths cannot disagree. Every problem is reported against a shape,
//! operation or member id, which each front end maps back to its sources.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::Diagnostics;
use crate::ir::{
    Binding, Bindings, Constraints, Example, Operation, Segment, Service, Shape, SimpleKind,
    Structure,
};
use crate::lower_examples::Check;
use crate::shape_id::ShapeId;

/// Headers the HTTP layer owns: framing and hop-by-hop headers (decision
/// 35c). A modelled member could only corrupt them.
const RESERVED_HEADERS: &[&str] = &[
    "connection",
    "content-length",
    "expect",
    "host",
    "keep-alive",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Every rule below, against `service`.
pub fn service(service: &Service, diagnostics: &mut Diagnostics) {
    operations(service, diagnostics);
    names(service, diagnostics);
    error_codes(service, diagnostics);
    for operation in &service.operations {
        self::operation(service, operation, diagnostics);
    }
    if service.protocol.uses_http_bindings() {
        routes(&service.operations, diagnostics);
    }
}

/// The rules of one operation, in the context of `service` (its shapes,
/// protocol and auth schemes), apart from those that compare operations.
pub fn operation(service: &Service, operation: &Operation, diagnostics: &mut Diagnostics) {
    errors(operation, diagnostics);
    if service.protocol.uses_http_bindings() {
        http(service, operation, diagnostics);
    }
    if let Some(paginated) = &operation.paginated {
        pagination(service, operation, paginated, diagnostics);
    }
    for (index, example) in operation.examples.iter().enumerate() {
        self::example(service, operation, index, example, diagnostics);
    }
}

/// The problems with `value` as a value of `target` under `constraints`,
/// such as a member's default; `what` names it in each message.
pub fn value(
    shapes: &BTreeMap<ShapeId, Shape>,
    what: &str,
    target: &ShapeId,
    constraints: &Constraints,
    value: &Value,
) -> Vec<String> {
    let mut check = Check {
        shapes,
        allow_constraint_errors: false,
        problems: Vec::new(),
    };
    check.value(what, target, constraints, value);
    check.problems
}

/// Why generated validators could not run `pattern`, an ECMA-262 regular
/// expression, if they could not.
pub fn pattern_problem(pattern: &str) -> Option<String> {
    match crate::pattern::rust(pattern) {
        Ok(translated) => regex::Regex::new(&translated).err().map(|e| e.to_string()),
        Err(problem) => Some(problem),
    }
}

fn operations(service: &Service, diagnostics: &mut Diagnostics) {
    let mut seen = BTreeSet::new();
    for operation in &service.operations {
        if !seen.insert(&operation.id) {
            diagnostics.push(
                &operation.id,
                format!("{} is part of the service twice", operation.name()),
            );
        }
    }
}

fn errors(operation: &Operation, diagnostics: &mut Diagnostics) {
    let mut seen = BTreeSet::new();
    for error in &operation.errors {
        if !seen.insert(error) {
            diagnostics.push(
                &operation.id,
                format!(
                    "{} returns `{}` twice; list each error once",
                    operation.name(),
                    error.name()
                ),
            );
        }
    }
}

/// Every emitter puts the service's shapes in one namespace (one module, one
/// `models.ts`), so two shapes must not share a name, nor differ only by
/// case, which collides on a case-insensitive file system.
fn names(service: &Service, diagnostics: &mut Diagnostics) {
    crate::names::check_unique(
        service
            .shapes
            .keys()
            .filter(|id| !id.is_prelude())
            .map(|id| (id, service.name_of(id))),
        diagnostics,
    );
}

/// Client error tables are service-wide and decode by the protocol's wire
/// code; renaming a shape changes its API name, not that discriminator.
fn error_codes(service: &Service, diagnostics: &mut Diagnostics) {
    let mut codes = BTreeMap::new();
    for (id, shape) in &service.shapes {
        if let Shape::Structure(structure) = shape
            && let Some(error) = &structure.error
            && let Some(previous) = codes.insert(&error.wire_code, id)
        {
            diagnostics.push(
                id,
                format!(
                    "error wire code {:?} collides with {previous}; client error decoding requires service-wide unique wire codes",
                    error.wire_code
                ),
            );
        }
    }
}

/// Label names do not distinguish routes; greediness and literal query
/// values do.
pub(crate) fn routes(operations: &[Operation], diagnostics: &mut Diagnostics) {
    let mut seen = BTreeMap::new();
    for operation in operations {
        let path: Vec<_> = operation
            .uri
            .segments
            .iter()
            .map(|segment| match segment {
                Segment::Literal(value) => format!("literal:{value}"),
                Segment::Label(name) if operation.uri.greedy_labels.contains(name) => {
                    "greedy".to_owned()
                }
                Segment::Label(_) => "label".to_owned(),
            })
            .collect();
        let mut query = operation.uri.query.clone();
        query.sort();
        let host = operation.endpoint.as_ref().map(|endpoint| {
            endpoint
                .parts
                .iter()
                .map(|part| match part {
                    crate::ir::HostPrefixPart::Literal { value } => format!("literal:{value}"),
                    crate::ir::HostPrefixPart::Label { .. } => "label".to_owned(),
                })
                .collect::<Vec<_>>()
        });
        if let Some(previous) = seen.insert((&operation.method, host, path, query), &operation.id) {
            diagnostics.push(
                &operation.id,
                format!("HTTP route conflicts with {previous}"),
            );
        }
    }
}

fn structure<'a>(service: &'a Service, id: Option<&ShapeId>) -> Option<&'a Structure> {
    match id.and_then(|id| service.shape(id)) {
        Some(Shape::Structure(structure)) => Some(structure),
        _ => None,
    }
}

/// What kind of text a binding can carry.
fn scalar(service: &Service, target: &ShapeId) -> bool {
    match service.shape(target) {
        Some(Shape::Simple(simple)) => {
            !matches!(simple.kind, SimpleKind::Document | SimpleKind::Blob)
        }
        Some(Shape::Enum(_) | Shape::IntEnum(_)) => true,
        _ => false,
    }
}

fn scalar_or_list(service: &Service, target: &ShapeId) -> bool {
    match service.shape(target) {
        Some(Shape::List(list)) => scalar(service, &list.member.target),
        _ => scalar(service, target),
    }
}

fn http(service: &Service, operation: &Operation, diagnostics: &mut Diagnostics) {
    let bearer = operation
        .auth
        .iter()
        .any(|scheme| service.is_bearer(scheme));
    if let Some(input) = structure(service, operation.input.as_ref()) {
        let owner = operation.input.as_ref().expect("checked");
        bindings(
            service,
            owner,
            input,
            &operation.request,
            bearer,
            diagnostics,
        );
        for label in &operation.request.labels {
            let Some(member) = input.member(label) else {
                continue;
            };
            let id = owner.with_member(label);
            if !member.required {
                diagnostics.push(&id, "a URI label member must be required");
            }
            if !scalar(service, &member.target) {
                diagnostics.push(
                    &id,
                    format!(
                        "a URI label carries text: `{}` cannot be a list, structure, map, blob or document",
                        member.name
                    ),
                );
            } else if operation.uri.greedy_labels.contains(label)
                && !matches!(
                    service.shape(&member.target),
                    Some(Shape::Enum(_))
                        | Some(Shape::Simple(crate::ir::Simple {
                            kind: SimpleKind::String,
                            ..
                        }))
                )
            {
                diagnostics.push(&id, "a greedy URI label must target a string");
            }
        }
        for binding in &operation.request.query {
            if let Some(member) = input.member(&binding.member)
                && !scalar_or_list(service, &member.target)
            {
                diagnostics.push(
                    &owner.with_member(&binding.member),
                    format!(
                        "a query parameter carries text: `{}` must be a scalar, an enum or a list of them",
                        member.name
                    ),
                );
            }
            if operation
                .uri
                .query
                .iter()
                .any(|(name, _)| name == &binding.name)
            {
                diagnostics.push(
                    &owner.with_member(&binding.member),
                    format!(
                        "the query parameter {:?} is also a literal in the URI",
                        binding.name
                    ),
                );
            }
        }
    }
    if let Some(output) = structure(service, operation.output.as_ref()) {
        let owner = operation.output.as_ref().expect("checked");
        bindings(
            service,
            owner,
            output,
            &operation.response,
            bearer,
            diagnostics,
        );
        // Decision 35c: these responses have no body on the wire.
        let bodyless = if operation.method == "HEAD" {
            Some("a HEAD response")
        } else if matches!(operation.status, 204 | 304) {
            Some(if operation.status == 204 {
                "a 204 response"
            } else {
                "a 304 response"
            })
        } else {
            None
        };
        if let Some(what) = bodyless {
            let members = operation
                .response
                .body
                .iter()
                .chain(operation.response.payload.iter().map(|p| &p.member));
            for member in members {
                diagnostics.push(
                    &owner.with_member(member),
                    format!("{what} carries no body: bind `{member}` to a header or remove it"),
                );
            }
        }
    }
    for error in &operation.errors {
        if let Some(Shape::Structure(shape)) = service.shape(error)
            && let Some(info) = &shape.error
        {
            let bindings = Bindings {
                headers: info.headers.clone(),
                prefix_headers: info.prefix_headers.clone(),
                ..Bindings::default()
            };
            headers(service, error, shape, &bindings, bearer, diagnostics);
        }
    }
}

/// Rules every input and output binding set obeys.
fn bindings(
    service: &Service,
    owner: &ShapeId,
    shape: &Structure,
    bindings: &Bindings,
    bearer: bool,
    diagnostics: &mut Diagnostics,
) {
    headers(service, owner, shape, bindings, bearer, diagnostics);
    let mut query = BTreeSet::new();
    for binding in &bindings.query {
        if !query.insert(&binding.name) {
            diagnostics.push(
                &owner.with_member(&binding.member),
                format!("the query parameter {:?} is bound twice", binding.name),
            );
        }
    }
    // A payload is the whole body: nothing else can travel in it.
    if let Some(payload) = &bindings.payload {
        for member in &bindings.body {
            diagnostics.push(
                &owner.with_member(member),
                format!(
                    "the body is the payload `{}`; bind `{member}` to a label, query parameter or header",
                    payload.member
                ),
            );
        }
    }
}

fn headers(
    service: &Service,
    owner: &ShapeId,
    shape: &Structure,
    bindings: &Bindings,
    bearer: bool,
    diagnostics: &mut Diagnostics,
) {
    let mut seen = BTreeSet::new();
    for Binding { member, name } in &bindings.headers {
        let id = owner.with_member(member);
        let lower = name.to_ascii_lowercase();
        if !seen.insert(lower.clone()) {
            diagnostics.push(&id, format!("the header {name:?} is bound twice"));
        }
        header_name(&id, name, &lower, bearer, diagnostics);
        if let Some(target) = shape.member(member)
            && !scalar_or_list(service, &target.target)
        {
            diagnostics.push(
                &id,
                format!(
                    "a header carries text: `{member}` must be a scalar, an enum, a timestamp or a list of them"
                ),
            );
        }
    }
    for Binding { member, name } in &bindings.prefix_headers {
        if !name.is_empty() && !is_token(name) {
            diagnostics.push(
                &owner.with_member(member),
                format!("{name:?} is not a valid header name prefix"),
            );
        }
    }
}

fn header_name(id: &ShapeId, name: &str, lower: &str, bearer: bool, diagnostics: &mut Diagnostics) {
    if !is_token(name) {
        diagnostics.push(id, format!("{name:?} is not a valid header name"));
    } else if RESERVED_HEADERS.contains(&lower) {
        diagnostics.push(
            id,
            format!("the HTTP layer owns the {name} header; it cannot be a modelled member"),
        );
    } else if bearer && lower == "authorization" {
        diagnostics.push(
            id,
            "the service authenticates with a bearer token in the Authorization header; it cannot be a modelled member",
        );
    }
}

/// RFC 9110 token characters.
fn is_token(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// A member a pagination path names, resolved through structures. Inputs
/// name a top-level member; outputs may name a nested one (`a.b`).
fn path_member<'a>(
    service: &'a Service,
    root: Option<&ShapeId>,
    path: &str,
    nested: bool,
) -> Option<&'a crate::ir::Member> {
    if !nested && path.contains('.') {
        return None;
    }
    // Each part is an identifier, even in a hand-written JSON AST.
    let identifier = |part: &str| {
        part.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if !path.split('.').all(identifier) {
        return None;
    }
    let mut structure = structure(service, root)?;
    let mut parts = path.split('.').peekable();
    while let Some(name) = parts.next() {
        let member = structure.member(name)?;
        if parts.peek().is_none() {
            return Some(member);
        }
        structure = match service.shape(&member.target) {
            Some(Shape::Structure(next)) => next,
            _ => return None,
        };
    }
    None
}

fn pagination(
    service: &Service,
    operation: &Operation,
    paginated: &crate::ir::Paginated,
    diagnostics: &mut Diagnostics,
) {
    let fields = [
        (
            "inputToken",
            Some(&paginated.input_token),
            operation.input.as_ref(),
            false,
        ),
        (
            "outputToken",
            Some(&paginated.output_token),
            operation.output.as_ref(),
            true,
        ),
        (
            "items",
            paginated.items.as_ref(),
            operation.output.as_ref(),
            true,
        ),
        (
            "pageSize",
            paginated.page_size.as_ref(),
            operation.input.as_ref(),
            false,
        ),
    ];
    for (key, path, root, nested) in fields {
        let Some(path) = path else { continue };
        let Some(member) = path_member(service, root, path, nested) else {
            diagnostics.push(
                &operation.id,
                format!("pagination {key} `{path}` does not name a member"),
            );
            continue;
        };
        let shape = service.shape(&member.target);
        let simple = |kinds: &[SimpleKind]| matches!(shape, Some(Shape::Simple(simple)) if kinds.contains(&simple.kind));
        let valid = match key {
            "items" => matches!(shape, Some(Shape::List(_) | Shape::Map(_))),
            "pageSize" => simple(&[
                SimpleKind::Byte,
                SimpleKind::Short,
                SimpleKind::Integer,
                SimpleKind::Long,
            ]),
            _ => simple(&[SimpleKind::String]) || matches!(shape, Some(Shape::Map(_))),
        };
        if !valid {
            let wanted = match key {
                "items" => "a list or map",
                "pageSize" => "an integer",
                _ => "a string",
            };
            diagnostics.push(
                &operation.id,
                format!("pagination {key} `{path}` must be {wanted}"),
            );
        }
        if key == "inputToken" && member.required {
            diagnostics.push(
                &operation.id,
                format!("pagination inputToken `{path}` must be optional: the first page has none"),
            );
        }
    }
}

fn example(
    service: &Service,
    operation: &Operation,
    index: usize,
    example: &Example,
    diagnostics: &mut Diagnostics,
) {
    let mut check = Check {
        shapes: &service.shapes,
        allow_constraint_errors: example.allow_constraint_errors,
        problems: Vec::new(),
    };
    // An omitted input is an empty one, so required members are still
    // demanded of it.
    let empty = Value::Object(serde_json::Map::new());
    check.io(
        "input",
        operation.input.as_ref(),
        example.input.as_ref().unwrap_or(&empty),
    );
    if let Some(output) = &example.output {
        check.io("output", operation.output.as_ref(), output);
    }
    if let Some(error) = &example.error {
        if example.output.is_some() {
            check
                .problems
                .push("an example cannot have both an output and an error".into());
        }
        if operation.errors.contains(&error.shape) {
            check.io("error", Some(&error.shape), &error.content);
        } else {
            check.problems.push(format!(
                "`{}` is not one of this operation's errors",
                error.shape.name()
            ));
        }
    }
    for problem in check.problems {
        diagnostics.push(
            &operation.id,
            format!("example {} {:?}: {problem}", index + 1, example.title),
        );
    }
}
