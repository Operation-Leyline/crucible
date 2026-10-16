//! The package's public surface (`crucible.sdks/1`), named by the functions
//! the client class and the model builders are emitted with.

use crucible_codegen_common::surface::{self, ClientType, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{
    Options, accessor, async_name, client_name, member_field, method, omittable, pages_async_name,
    pages_name, ty, unset_field,
};

/// What the generated package shows its callers: the Maven coordinate and
/// Java package, the client class, and per operation its blocking and
/// asynchronous methods, paginators, and the models it takes (through
/// `Model.builder()`) and returns (through accessors).
///
/// A snippet imports the client and `ClientConfig`. Models keep their
/// `<package>.model.` qualifier, so a model named like a `java.lang` class
/// stays unambiguous, and other platform types keep the qualifier the
/// generated code writes (`java.util.List`, `java.time.Instant`).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = client_name(service);
    let package = &options.package_name;
    // `ClientConfig` sets its endpoint through a public field, so filling
    // it takes a statement.
    let construct = if options.endpoint.is_some() {
        format!("{client} client = new {client}(new ClientConfig());")
    } else {
        format!(
            "ClientConfig config = new ClientConfig();\nconfig.endpoint = endpoint;\n{client} client = new {client}(config);"
        )
    };
    let mut constructor = ClientType::new(&client, construct);
    constructor.statements = true;
    let mut out = surface::Surface::new(
        "java",
        Package::new(
            "maven",
            format!("{}:{}", options.group_id, options.artifact_id),
            package,
        ),
        constructor,
    );
    out.package.imports = vec![
        format!("import {package}.ClientConfig;"),
        format!("import {package}.{client};"),
    ];
    out.package.qualifier = format!("{package}.model.");
    out.endpoint.clone_from(&options.endpoint);
    for operation in &service.operations {
        let mut entry = Operation::new(method(operation.name()));
        entry.async_method = Some(async_name(operation.name()));
        entry.paginators = operation.paginated.as_ref().map(|_| {
            vec![
                pages_name(operation.name()),
                pages_async_name(operation.name()),
            ]
        });
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, id, package, true));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, id, package, false));
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// A type as the generated code spells it, less `java.lang.`, which every
/// Java file imports.
fn caller_type(service: &Service, id: &ShapeId, package: &str) -> String {
    without_java_lang(&ty(service, id, package))
}

/// `ty` without the `java.lang.` that starts a type name: the whole type or
/// a generic argument, never the same letters inside a package such as
/// `org.java.lang.weather`.
fn without_java_lang(ty: &str) -> String {
    const PREFIX: &str = "java.lang.";
    let mut out = String::with_capacity(ty.len());
    let mut rest = ty;
    let mut boundary = true;
    while let Some(c) = rest.chars().next() {
        if boundary && rest.starts_with(PREFIX) {
            rest = &rest[PREFIX.len()..];
            boundary = false;
            continue;
        }
        boundary = matches!(c, '<' | ',' | ' ' | '[' | '?' | '&');
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// An input as its builder sets it, or an output as its accessors read it.
fn structure(service: &Service, id: &ShapeId, package: &str, input: bool) -> Structure {
    let style = if input { "builder" } else { "methods" };
    let mut structure = Structure::new(caller_type(service, id, package), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                let field = member_field(&member.name, package);
                let name = if input {
                    field
                } else {
                    accessor(&field, shape.error.is_some())
                };
                // A builder may leave out what starts unset or at its
                // default; a model lacks only what is an `OptionalValue`.
                let optional = if input {
                    omittable(member, false)
                } else {
                    unset_field(member, false)
                };
                Member::new(
                    member,
                    name,
                    caller_type(service, &member.target, package),
                    optional,
                )
            })
            .collect();
    }
    structure
}

#[cfg(test)]
mod tests {
    use super::without_java_lang;

    #[test]
    fn only_a_leading_java_lang_is_dropped() {
        assert_eq!(without_java_lang("java.lang.String"), "String");
        assert_eq!(
            without_java_lang("java.util.Map<java.lang.String, java.lang.Long>"),
            "java.util.Map<String, Long>"
        );
        assert_eq!(
            without_java_lang("org.java.lang.weather.model.City"),
            "org.java.lang.weather.model.City"
        );
        assert_eq!(
            without_java_lang("java.util.List<org.java.lang.weather.model.City>"),
            "java.util.List<org.java.lang.weather.model.City>"
        );
    }
}
