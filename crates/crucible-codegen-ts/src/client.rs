//! `client.ts`: the client class, one method per operation and a paginator
//! per paginated operation.

use std::collections::{BTreeMap, BTreeSet};

use crucible_ir::ShapeId;
use crucible_ir::ir::{Operation, Service, Shape};

use crate::names::{Links, doc, documented_linked, literal, method_name, optional};
use crate::{Middleware, Options};

pub(crate) fn emit(service: &Service, options: &Options, links: &Links<'_>) -> String {
    let client = links.client;
    let links = links.naming("m.", "", crate::names::ERRORS_SCOPE);
    let mut out = String::new();
    documented_linked(
        &mut out,
        &links,
        "",
        Some(&format!(
            "{}\n\nFetch-based HTTP client. Construct with ClientConfig. Calls may run concurrently when custom fetch, middleware and credential providers support concurrency. The client owns no transport pool and has no close method; callers manage custom fetch resources.",
            service.docs.as_deref().unwrap_or("")
        )),
        &Default::default(),
    );
    out.push_str(&format!(
        "export class {client} {{\n  readonly #transport: Transport;\n\n"
    ));
    let endpoint = options
        .endpoint
        .as_deref()
        .map_or("undefined".to_owned(), literal);
    let middleware: Vec<&str> = options
        .middleware
        .iter()
        .map(|m| match m {
            Middleware::ContentSha256 => "contentSha256",
        })
        .collect();
    let bearer = service.auth.iter().any(|id| service.is_bearer(id));
    doc(
        &mut out,
        "  ",
        Some(
            "Configure endpoint, authentication, retries, timeout and fetch middleware. Uses global fetch when no fetch is supplied.",
        ),
    );
    out.push_str(&format!(
        "  constructor(config: ClientConfig = {{}}) {{\n    \
         this.#transport = new Transport(config, {{ endpoint: {endpoint}, middleware: [{}], bearer: {bearer} }}, spec.errors);\n  }}\n",
        middleware.join(", ")
    ));
    let names = method_names(service);
    for operation in &service.operations {
        out.push('\n');
        let (name, paginator_name) = &names[&operation.id];
        method(&mut out, &links, operation, name);
        if operation.paginated.is_some() {
            out.push('\n');
            paginator(
                &mut out,
                service,
                operation,
                name,
                paginator_name.as_deref().unwrap(),
            );
        }
    }
    out.push_str("}\n");

    let mut runtime = vec!["Transport"];
    if out.contains("paginate(") {
        runtime.push("paginate");
    }
    runtime.extend(middleware);
    runtime.sort_unstable();
    let mut header = String::from(crate::HEADER);
    header.push_str(&format!(
        "import {{ {} }} from \"./runtime.js\";\n",
        runtime.join(", ")
    ));
    header.push_str("import type { ClientConfig, RequestOptions } from \"./runtime.js\";\n");
    if out.contains("m.") {
        header.push_str("import type * as m from \"./models.js\";\n");
    }
    header.push_str(&crate::names::doc_imports(&out, false));
    header.push_str("import * as spec from \"./schemas.js\";\n\n");
    header + &out
}

/// Reserve natural operation names before assigning suffixes or paginator
/// helpers, so adding a helper never hides an ordinary modeled operation.
pub(crate) fn method_names(service: &Service) -> BTreeMap<ShapeId, (String, Option<String>)> {
    let mut operations: Vec<_> = service.operations.iter().collect();
    operations.sort_by_key(|op| &op.id);
    let mut reserved: BTreeSet<_> = operations.iter().map(|op| method_name(op.name())).collect();
    let mut used = BTreeSet::from(["constructor".to_owned()]);
    let mut names = BTreeMap::new();
    for op in &operations {
        let name = allocate(method_name(op.name()), &reserved, &mut used);
        names.insert(op.id.clone(), (name, None));
    }
    reserved.extend(
        operations
            .iter()
            .filter(|op| op.paginated.is_some())
            .map(|op| format!("paginate{}", op.name())),
    );
    for op in operations.into_iter().filter(|op| op.paginated.is_some()) {
        let name = allocate(format!("paginate{}", op.name()), &reserved, &mut used);
        names.get_mut(&op.id).unwrap().1 = Some(name);
    }
    names
}

fn allocate(base: String, reserved: &BTreeSet<String>, used: &mut BTreeSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }
    for suffix in 2.. {
        let name = format!("{base}_{suffix}");
        if !reserved.contains(&name) && used.insert(name.clone()) {
            return name;
        }
    }
    unreachable!()
}

/// `(input: m.XInput, options?)`, with `input` defaulting to `{}` when every
/// member is optional.
fn parameters(service: &Service, operation: &Operation) -> (String, String) {
    let Some(id) = &operation.input else {
        return ("options?: RequestOptions".to_owned(), "{}".to_owned());
    };
    let all_optional = match service.shape(id) {
        Some(Shape::Structure(input)) => input.members.iter().all(|m| optional(input, m)),
        _ => false,
    };
    let default = if all_optional { " = {}" } else { "" };
    (
        format!(
            "input: m.{}{default}, options?: RequestOptions",
            service.name_of(id)
        ),
        "input".to_owned(),
    )
}

fn output_type(service: &Service, operation: &Operation) -> String {
    operation
        .output
        .as_ref()
        .map_or("void".to_owned(), |id| format!("m.{}", service.name_of(id)))
}

fn method(out: &mut String, links: &Links<'_>, operation: &Operation, name: &str) {
    let service = links.service;
    documented_linked(
        out,
        links,
        "  ",
        Some(&crucible_codegen_common::documentation::operation(
            service, operation,
        )),
        &operation.notes,
    );
    let (params, input) = parameters(service, operation);
    out.push_str(&format!(
        "  {}({params}): Promise<{}> {{\n    return this.#transport.send(spec.{}, {input}, options);\n  }}\n",
        name,
        output_type(service, operation),
        operation.name(),
    ));
}

fn paginator(
    out: &mut String,
    service: &Service,
    operation: &Operation,
    name: &str,
    paginator_name: &str,
) {
    let paginated = operation.paginated.as_ref().expect("checked by the caller");
    out.push_str(&format!(
        "  /** Every page of {}, following `{}` until absent or empty; rejects repeated tokens. Input is snapshotted. */\n",
        operation.name(),
        paginated.output_token
    ));
    let (params, _) = parameters(service, operation);
    let path: Vec<String> = paginated.output_token.split('.').map(literal).collect();
    out.push_str(&format!(
        "  {paginator_name}({params}): AsyncGenerator<{}, void, undefined> {{\n    \
         return paginate(page => this.{name}(page, options), input, {}, [{}]);\n  }}\n",
        output_type(service, operation),
        literal(&paginated.input_token),
        path.join(", "),
    ));
}
