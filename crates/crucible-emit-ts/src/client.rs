//! `client.ts`: the client class, one method per operation and a paginator
//! per paginated operation.

use crucible_core::ir::{Operation, Service, Shape};

use crate::names::{doc, literal, method_name, optional, smithy};
use crate::{Middleware, Options};

pub(crate) fn emit(service: &Service, options: &Options, client: &str) -> String {
    let mut out = String::new();
    doc(&mut out, "", service.docs.as_deref());
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
    let bearer = service.auth.contains(&smithy("httpBearerAuth"));
    out.push_str(&format!(
        "  constructor(config: ClientConfig = {{}}) {{\n    \
         this.#transport = new Transport(config, {{ endpoint: {endpoint}, middleware: [{}], bearer: {bearer} }}, spec.errors);\n  }}\n",
        middleware.join(", ")
    ));
    for operation in &service.operations {
        out.push('\n');
        method(&mut out, service, operation);
        if operation.paginated.is_some() {
            out.push('\n');
            paginator(&mut out, service, operation);
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
    header.push_str("import * as spec from \"./schemas.js\";\n\n");
    header + &out
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

fn method(out: &mut String, service: &Service, operation: &Operation) {
    doc(out, "  ", operation.docs.as_deref());
    let (params, input) = parameters(service, operation);
    out.push_str(&format!(
        "  {}({params}): Promise<{}> {{\n    return this.#transport.send(spec.{}, {input}, options);\n  }}\n",
        method_name(operation.name()),
        output_type(service, operation),
        operation.name(),
    ));
}

fn paginator(out: &mut String, service: &Service, operation: &Operation) {
    let paginated = operation.paginated.as_ref().expect("checked by the caller");
    let name = method_name(operation.name());
    out.push_str(&format!(
        "  /** Every page of {}, following `{}` until a page comes back without one. */\n",
        operation.name(),
        paginated.output_token
    ));
    let (params, _) = parameters(service, operation);
    let path: Vec<String> = paginated.output_token.split('.').map(literal).collect();
    out.push_str(&format!(
        "  paginate{}({params}): AsyncGenerator<{}, void, undefined> {{\n    \
         return paginate(page => this.{name}(page, options), input, {}, [{}]);\n  }}\n",
        operation.name(),
        output_type(service, operation),
        literal(&paginated.input_token),
        path.join(", "),
    ));
}
