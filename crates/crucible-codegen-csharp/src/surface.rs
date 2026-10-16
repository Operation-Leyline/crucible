//! The NuGet package's public surface (`crucible.sdks/1`), named by the
//! functions the client class and models are emitted with.

use crucible_codegen_common::surface::{self, ClientType, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{
    Options, async_method_name, client_name, field, method_name, omittable, pages_method_name,
    presence, ty,
};

/// What the generated package shows its callers: the NuGet package, the
/// client class, and per operation its blocking and asynchronous methods,
/// the page walker, and the models it takes (an object initializer) and
/// returns (properties).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let ns = &options.namespace;
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("new {client}(new ClientConfig())")
    } else {
        format!("new {client}(new ClientConfig {{ Endpoint = endpoint }})")
    };
    let mut package = Package::new("nuget", &options.package_id, ns);
    // The client and runtime live in the root namespace, the models in
    // `.Models`; platform types stay `System.`-qualified, as emitted.
    package.imports = vec![format!("using {ns};"), format!("using {ns}.Models;")];
    let mut out = surface::Surface::new("csharp", package, ClientType::new(client, construct));
    out.endpoint.clone_from(&options.endpoint);
    for op in &service.operations {
        let mut entry = Operation::new(identifier(&method_name(op)));
        entry.async_method = Some(identifier(&async_method_name(op)));
        entry.paginators = pages_method_name(op).map(|name| vec![identifier(&name)]);
        entry.input = op
            .input
            .as_ref()
            .map(|id| structure(service, id, ns, "fields"));
        entry.output = op
            .output
            .as_ref()
            .map(|id| structure(service, id, ns, "properties"));
        out.operations.insert(op.id.to_string(), entry);
    }
    out
}

/// A model class as `Models/<Name>.cs` declares it: a property per member.
fn structure(service: &Service, id: &ShapeId, ns: &str, style: &str) -> Structure {
    let mut structure = Structure::new(caller_type(&ty(service, id, ns), ns), style);
    if let Some(model @ Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                let mut entry = Member::new(
                    member,
                    identifier(&field(&member.name)),
                    caller_type(&ty(service, &member.target, ns), ns),
                );
                // An input may leave out what the client defaults; an
                // output lacks only what has a presence.
                entry.optional = if style == "fields" {
                    omittable(model, member)
                } else {
                    presence(model, member)
                };
                entry
            })
            .collect();
    }
    structure
}

/// A type as a caller writes it after the package's `using` lines:
/// `global::Ns.Models.@City` reads `City`, `global::System.DateTimeOffset`
/// reads `System.DateTimeOffset`. The generator writes every model name
/// verbatim (`@City`); the `@` stays only where the name is a keyword
/// (`@class`), which needs it.
fn caller_type(ty: &str, ns: &str) -> String {
    let ty = ty
        .replace(&format!("global::{ns}.Models."), "")
        .replace(&format!("global::{ns}."), "")
        .replace("global::", "");
    let mut out = String::with_capacity(ty.len());
    let mut rest = ty.as_str();
    while let Some(at) = rest.find('@') {
        out.push_str(&rest[..at]);
        let name = &rest[at + 1..];
        let end = name
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(name.len());
        out.push_str(&identifier(&name[..end]));
        rest = &name[end..];
    }
    out.push_str(rest);
    out
}

/// `name` as C# source writes it: `@`-escaped only when it is a keyword.
fn identifier(name: &str) -> String {
    if KEYWORDS.contains(&name) {
        format!("@{name}")
    } else {
        name.to_owned()
    }
}

/// C#'s reserved keywords, which an identifier spells with `@`. Contextual
/// keywords (`var`, `record`, `async`) are ordinary identifiers.
const KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "checked",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "float",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "lock",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sbyte",
    "sealed",
    "short",
    "sizeof",
    "stackalloc",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "uint",
    "ulong",
    "unchecked",
    "unsafe",
    "ushort",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];
