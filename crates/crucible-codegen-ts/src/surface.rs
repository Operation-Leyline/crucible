//! The client's public surface (`crucible.sdks/1`), named by the functions
//! `client.ts` and `models.ts` are emitted with.

use crucible_codegen_common::surface::{
    self, ClientType, Member, Middleware, Operation, Package, Structure,
};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::Options;
use crate::client::method_names;
use crate::names::{literal, optional, property, ts_type};

/// The namespace a snippet imports the package's types under. Every
/// generated name lives in it, so none can collide with the snippet's own.
const MODELS: &str = "models";

/// What the generated package shows its callers: the npm package, the
/// client class, and per operation its method, paginator, and the
/// interfaces it takes and returns.
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = crate::client_name(service, options);
    let construct = if options.endpoint.is_some() {
        format!("new {client}()")
    } else {
        format!("new {client}({{ endpoint }})")
    };
    let mut package = Package::new("npm", &options.package_name, &options.package_name);
    let specifier = literal(&options.package_name);
    package.imports = vec![
        format!("import {{ {client} }} from {specifier};"),
        format!("import type * as {MODELS} from {specifier};"),
    ];
    package.qualifier = format!("{MODELS}.");
    let mut out = surface::Surface::new("ts", package, ClientType::new(client, construct));
    out.endpoint.clone_from(&options.endpoint);
    out.middleware = options
        .middleware
        .iter()
        .map(|middleware| match middleware {
            crate::Middleware::ContentSha256 => Middleware::content_sha256(),
        })
        .collect();
    let names = method_names(service);
    for operation in &service.operations {
        let (method, paginator) = &names[&operation.id];
        let mut entry = Operation::new(method);
        entry.paginators = paginator.clone().map(|name| vec![name]);
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, id, "object"));
        entry.output = operation.output.as_ref().map(|id| {
            if operation.response_mode == crucible_ir::ir::ResponseMode::ServerStream {
                Structure::new(
                    format!("{MODELS}.ResponseStream<{}>", caller_type(service, id)),
                    "async-iterator",
                )
            } else {
                structure(service, id, "properties")
            }
        });
        let call = crucible_codegen_common::snippet::call(service, operation);
        (entry.snippet, entry.paginated_snippet) = crate::snippet::snippets(
            service,
            options,
            &out.middleware,
            &call,
            method,
            paginator.as_deref(),
        );
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// `target`'s type as a snippet writes it: model and runtime types in the
/// package's namespace, globals by their own name. The generated files
/// reach globals through `globalThis.` (a model may be called `Date`) and
/// import the runtime's types unqualified; a snippet does neither.
fn caller_type(service: &Service, target: &ShapeId) -> String {
    let qualifier = format!("{MODELS}.");
    let ty = ts_type(service, target, &qualifier);
    match ty.strip_prefix("globalThis.") {
        Some(global) => global.to_owned(),
        None if ty.starts_with(&qualifier) || !ty.starts_with(char::is_uppercase) => ty,
        // `Unit`, `Document` and `Decimal`: the runtime's, which the
        // package re-exports.
        None => format!("{qualifier}{ty}"),
    }
}

/// An interface as `models.ts` declares it: a property per member.
fn structure(service: &Service, id: &ShapeId, style: &str) -> Structure {
    let mut structure = Structure::new(format!("{MODELS}.{}", service.name_of(id)), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                // The interface's own `?`: an input may omit what is not
                // `@required` (a default is the server's to apply), and an
                // output follows client nullability.
                Member::new(
                    member,
                    property(&member.name),
                    caller_type(service, &member.target),
                    optional(shape, member),
                )
            })
            .collect();
    }
    structure
}
