//! Rules that hold of every lowered service, whichever front end built it.
//!
//! Each front end parses its own syntax and reports problems in its own
//! terms; what remains true of the IR itself (routes are unique, bindings
//! carry text, pagination members have the right types, examples fit their
//! shapes, names do not collide) is checked here, once, on the IR. Every
//! problem is reported against a shape, operation or member id, which the
//! front end maps back to its sources.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::Diagnostics;
use crate::ir::{
    Binding, Bindings, Constraints, Example, Operation, Segment, Service, Shape, SimpleKind,
    Structure, Uri,
};
use crate::lower_examples::Check;
use crate::shape_id::ShapeId;

mod authorization;

pub use authorization::{
    FindingSeverity, PermissionFacts, PermissionFinding, attaches_not_found,
    derived_authorization_name, needs_not_found, not_found_id, permission_value_valid, permissions,
    permissions_from_ir,
};

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
    recursion(service, diagnostics);
    operations(service, diagnostics);
    names(service, diagnostics);
    error_codes(service, diagnostics);
    for operation in &service.operations {
        self::operation(service, operation, diagnostics);
    }
    routes(&service.operations, diagnostics);
    sender_lifecycle(service, diagnostics);
}

/// A resource's lifecycle is what clients and infrastructure call to
/// manage it, so no sender-signed operation (decision 81) holds a slot:
/// Terraform's provider would call it unsigned.
fn sender_lifecycle(service: &Service, diagnostics: &mut Diagnostics) {
    for resource in service.resources.values() {
        let lifecycle = &resource.lifecycle;
        for (role, slot) in [
            ("creates", &lifecycle.create),
            ("puts", &lifecycle.put),
            ("reads", &lifecycle.read),
            ("updates", &lifecycle.update),
            ("deletes", &lifecycle.delete),
            ("lists", &lifecycle.list),
        ] {
            let Some(operation) = slot
                .as_ref()
                .and_then(|id| service.operations.iter().find(|op| &op.id == id))
            else {
                continue;
            };
            if let Some(sender) = &operation.sender {
                diagnostics.push(
                    &operation.id,
                    format!(
                        "sender.lifecycle: {} {role} {}, so clients and infrastructure call it to manage the resource, but {} calls it: bind it to no lifecycle role (`on {}`, or no resource)",
                        operation.name(),
                        resource.id.name(),
                        sender.name,
                        resource.id.name()
                    ),
                );
            }
        }
    }
}

/// The rules of one operation, in the context of `service` (its shapes,
/// protocol and auth schemes), apart from those that compare operations.
pub fn operation(service: &Service, operation: &Operation, diagnostics: &mut Diagnostics) {
    errors(operation, diagnostics);
    sender(operation, diagnostics);
    http(service, operation, diagnostics);
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

/// The limits on recursive shapes, so that every shape has a finite value
/// and every target language can spell its types:
/// - a cycle of lists and maps alone is a type most languages cannot spell;
/// - a structure must not reach itself through required members alone,
///   where each step is a required member targeting a structure;
/// - a union must terminate: some member reaches a value without coming
///   back, where a scalar, list or map terminates, a structure terminates
///   when it has an optional member or a required one that terminates, and
///   a member already followed does not.
///
/// Only shapes in a recursive group can break these, so only they are
/// checked.
fn recursion(service: &Service, diagnostics: &mut Diagnostics) {
    for group in &service.recursive_groups {
        if group
            .shapes
            .iter()
            .all(|id| matches!(service.shape(id), Some(Shape::List(_) | Shape::Map(_))))
        {
            for id in &group.shapes {
                diagnostics.push(
                    id,
                    "recursive collections must pass through a structure or union; a collection-only cycle is invalid",
                );
            }
        }
    }
    for id in service
        .recursive_groups
        .iter()
        .flat_map(|group| &group.shapes)
    {
        match service.shape(id) {
            Some(Shape::Structure(_)) if required_cycle(service, id) => diagnostics.push(
                id,
                "this structure reaches itself through required members alone, so no finite value of it exists; make one of them optional",
            ),
            Some(Shape::Union(_)) if !terminates(service, id, &mut BTreeSet::new()) => {
                diagnostics.push(
                    id,
                    "no finite value of this union exists: every member leads back to it; add a member that does not, or one that goes through a list, map or optional member",
                )
            }
            _ => {}
        }
    }
}

/// Whether `start` reaches itself along required members that target
/// structures.
fn required_cycle(service: &Service, start: &ShapeId) -> bool {
    let next = |id: &ShapeId| -> Vec<ShapeId> {
        match service.shape(id) {
            Some(Shape::Structure(s)) => s
                .members
                .iter()
                .filter(|member| member.required)
                .filter(|member| matches!(service.shape(&member.target), Some(Shape::Structure(_))))
                .map(|member| member.target.clone())
                .collect(),
            _ => Vec::new(),
        }
    };
    let mut seen = BTreeSet::new();
    let mut stack = next(start);
    while let Some(id) = stack.pop() {
        if &id == start {
            return true;
        }
        if seen.insert(id.clone()) {
            stack.extend(next(&id));
        }
    }
    false
}

/// Whether a value of `id` can be built
/// without following a member twice (`visited` holds member ids).
fn terminates(service: &Service, id: &ShapeId, visited: &mut BTreeSet<ShapeId>) -> bool {
    let member =
        |owner: &ShapeId, name: &str, target: &ShapeId, visited: &mut BTreeSet<ShapeId>| {
            visited.insert(owner.with_member(name)) && terminates(service, target, visited)
        };
    match service.shape(id) {
        Some(Shape::Structure(s)) => {
            s.members.is_empty()
                || s.members.iter().any(|m| !m.required)
                || s.members
                    .iter()
                    .filter(|m| m.required)
                    .any(|m| member(id, &m.name, &m.target, visited))
        }
        Some(Shape::Union(s)) => s
            .members
            .iter()
            .any(|m| member(id, &m.name, &m.target, visited)),
        _ => true,
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

/// A sender-signed operation (decision 81) is called by a third party that
/// holds no credential of this service, so it is `public`, and its
/// signature is a modelled request header.
fn sender(operation: &Operation, diagnostics: &mut Diagnostics) {
    let Some(sender) = &operation.sender else {
        return;
    };
    if sender.name.trim().is_empty() {
        diagnostics.push(
            &operation.id,
            format!(
                "sender.name: `@sender` on {} names no sender; name who calls it, such as `@sender(\"Stripe\", signature: signature)`",
                operation.name()
            ),
        );
    }
    if !operation.public || !operation.auth.is_empty() {
        diagnostics.push(
            &operation.id,
            format!(
                "sender.public: {} is called by {}, which holds no credential of this service: declare it `public`, and let its signature authenticate it",
                operation.name(),
                sender.name
            ),
        );
    }
    if operation.signature_header().is_none() {
        diagnostics.push(
            &operation.id,
            format!(
                "sender.signature: `signature: {}` on {} names no input member bound with `@header`; the signature travels in a request header the model declares",
                sender.signature,
                operation.name()
            ),
        );
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

/// What tells a route apart from another: its method, its path with label
/// names left out but greediness kept, and its literal query values.
fn route_key<'a>(method: &'a str, uri: &Uri) -> RouteKey<'a> {
    let path = uri
        .segments
        .iter()
        .map(|segment| match segment {
            Segment::Literal(value) => format!("literal:{value}"),
            Segment::Label(name) if uri.greedy_labels.contains(name) => "greedy".to_owned(),
            Segment::Label(_) => "label".to_owned(),
        })
        .collect();
    let mut query = uri.query.clone();
    query.sort();
    (method, path, query)
}

type RouteKey<'a> = (&'a str, Vec<String>, Vec<(String, Option<String>)>);

/// Whether two routes answer the same requests, as [`service`] compares
/// an operation's with every other's: `GET "/items/{id}"` and
/// `GET "/items/{name}"` do.
pub fn same_route(method: &str, uri: &Uri, other_method: &str, other: &Uri) -> bool {
    route_key(method, uri) == route_key(other_method, other)
}

/// Label names do not distinguish routes; greediness and literal query
/// values do.
pub(crate) fn routes(operations: &[Operation], diagnostics: &mut Diagnostics) {
    let mut seen = BTreeMap::new();
    for operation in operations {
        if let Some(previous) =
            seen.insert(route_key(&operation.method, &operation.uri), &operation.id)
        {
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
                ..Bindings::default()
            };
            headers(service, error, shape, &bindings, bearer, diagnostics);
            // An error's body is every member not bound elsewhere; beside a
            // payload, those members would be dropped.
            if let Some(payload) = &info.payload {
                let bound = |name: &str| {
                    name == payload.member || info.headers.iter().any(|b| b.member == name)
                };
                for member in shape.members.iter().filter(|m| !bound(&m.name)) {
                    diagnostics.push(
                        &error.with_member(&member.name),
                        format!(
                            "the body is the payload `{}`; bind `{}` to a header",
                            payload.member, member.name
                        ),
                    );
                }
            }
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
    for problem in example_problems(service, operation, example) {
        diagnostics.push(
            &operation.id,
            format!("example {} {:?}: {problem}", index + 1, example.title),
        );
    }
}

/// Why `example` does not hold for `operation` in `service`, if it does
/// not: what [`operation`] reports about it, without its position.
pub(crate) fn example_problems(
    service: &Service,
    operation: &Operation,
    example: &Example,
) -> Vec<String> {
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
    check.problems
}
