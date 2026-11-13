//! `roles.h` and `roles.c` (docs/roles.md §7 "Clients"): the service's
//! role and scope values as open string types, `weather_role_value` and
//! `weather_role_scope_value`, with a `_known_` function per value the way
//! an enum has one per value, and `weather_roles`, the registry a console's
//! role picker reads. Generated from `crucible_codegen_common::roles`' one
//! rule, as every client's is: public roles and scopes only, in catalogue
//! order, and only public permissions in a role's `permissions`. Every
//! symbol shares C's one namespace with the models and operations, so
//! `lib.rs` diagnoses a collision through [`symbols`].

use crate::{File, Options, names};
use crucible_codegen_common::permissions::{code_span, constant_symbol};
use crucible_codegen_common::roles::{ClientRole, client_roles, client_scopes, custom};
use crucible_ir::ShapeId;
use crucible_ir::ir::{DocNotes, Service};

/// `weather_role_value`: never a model's `weather_role`, which a model
/// `Role` (a `@roleTypes` enum) declares.
fn role_type(prefix: &str) -> String {
    format!("{prefix}_role_value")
}

/// `weather_role_scope_value`.
fn scope_type(prefix: &str) -> String {
    format!("{prefix}_role_scope_value")
}

/// The constant function for one value of an open type.
fn known(native: &str, symbol: &str) -> String {
    format!("{native}_known_{}", names::field(&constant_symbol(symbol)))
}

/// Every symbol the role files declare, each with the shape a collision is
/// reported against (the service) and what it names.
pub fn symbols(service: &Service, prefix: &str) -> Vec<(ShapeId, String, String)> {
    let roles = client_roles(service);
    if roles.is_empty() {
        return Vec::new();
    }
    let mut result: Vec<(ShapeId, String, String)> = [
        "role_permission",
        "role_description",
        "roles",
        "roles_count",
    ]
    .into_iter()
    .map(|name| {
        (
            service.id.clone(),
            format!("{prefix}_{name}"),
            "the role registry".to_owned(),
        )
    })
    .collect();
    let roles_type = role_type(prefix);
    result.push((
        service.id.clone(),
        roles_type.clone(),
        "the role type".to_owned(),
    ));
    for role in &roles {
        result.push((
            service.id.clone(),
            known(&roles_type, &role.role.symbol),
            format!("role `{}`", role.role.symbol),
        ));
    }
    let scopes_type = scope_type(prefix);
    result.push((
        service.id.clone(),
        scopes_type.clone(),
        "the scope type".to_owned(),
    ));
    for scope in client_scopes(service) {
        result.push((
            service.id.clone(),
            known(&scopes_type, &scope.scope.symbol),
            format!("scope `{}`", scope.scope.symbol),
        ));
    }
    result
}

/// One open string type and its `_known_` functions.
fn open_type(
    service: &Service,
    prefix: &str,
    native: &str,
    what: &str,
    values: &[(&str, &str, Option<&str>, &DocNotes)],
) -> String {
    let mut out = format!(
        "\n{}",
        crucible_codegen_common::doxygen(
            &format!(
                "The {what} values of `{}`, by the values bindings store: a known one, or any string a newer server writes, kept as it is. A value is a borrowed view.",
                service.name_of(&service.id)
            ),
            "",
        )
    );
    out.push_str(&format!("typedef {prefix}_string {native};\n"));
    for (symbol, value, docs, notes) in values {
        // Model text: `doxygen_for` escapes it like every other model
        // comment, so `*/` closes nothing and `@param` is no command.
        let mut text = docs.map(str::to_owned).unwrap_or_default();
        if !text.trim().is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&format!("{}.", code_span(value)));
        out.push_str(&crucible_codegen_common::doxygen_for(
            crate::links::render(service, Some(text.as_str()), prefix).as_deref(),
            notes,
            "",
        ));
        out.push_str(&format!(
            "static inline {native} {}(void) {{ {native} result = {}; return result; }}\n",
            known(native, symbol),
            names::view(value)
        ));
    }
    out
}

/// A static array of string views and its length, or `0, NULL` when empty
/// (C has no empty array).
fn views(source: &mut String, prefix: &str, name: &str, values: &[&str]) -> String {
    if values.is_empty() {
        return "0, NULL".to_owned();
    }
    source.push_str(&format!(
        "static const {prefix}_string {name}[] = {{{}}};\n",
        values
            .iter()
            .map(|v| names::view(v))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    format!("{}, {name}", values.len())
}

fn entry(source: &mut String, prefix: &str, index: usize, role: &ClientRole<'_>) -> String {
    let scopes = views(source, prefix, &format!("scopes_{index}"), &role.scopes);
    let holders: Vec<&str> = role.holders.iter().map(String::as_str).collect();
    let holders = views(source, prefix, &format!("holders_{index}"), &holders);
    let assigned_by = views(
        source,
        prefix,
        &format!("assigned_by_{index}"),
        &role.assigned_by,
    );
    let permissions = if role.permissions.is_empty() {
        "0, NULL".to_owned()
    } else {
        source.push_str(&format!(
            "static const {prefix}_role_permission permissions_{index}[] = {{\n"
        ));
        for permission in &role.permissions {
            source.push_str(&format!(
                "    {{{}, {}}},\n",
                names::view(permission.resource_type),
                names::view(permission.value)
            ));
        }
        source.push_str("};\n");
        format!("{}, permissions_{index}", role.permissions.len())
    };
    format!(
        "    {{{}, {}, {scopes}, {holders}, {assigned_by}, {permissions}}},\n",
        names::view(role.value),
        names::view(role.description)
    )
}

pub fn emit(service: &Service, options: &Options) -> Vec<File> {
    let roles = client_roles(service);
    if roles.is_empty() {
        return Vec::new();
    }
    let scopes = client_scopes(service);
    let p = &options.prefix;
    let guard = format!("{}_ROLES_H", p.to_ascii_uppercase());
    let mut header = format!(
        "#ifndef {guard}\n#define {guard}\n#include \"runtime/types.h\"\n\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n"
    );
    let role_values: Vec<(&str, &str, Option<&str>, &DocNotes)> = roles
        .iter()
        .map(|r| {
            (
                r.role.symbol.as_str(),
                r.value,
                r.role.docs.as_deref(),
                &r.role.notes,
            )
        })
        .collect();
    header.push_str(&open_type(service, p, &role_type(p), "role", &role_values));
    let scope_values: Vec<(&str, &str, Option<&str>, &DocNotes)> = scopes
        .iter()
        .map(|s| {
            (
                s.scope.symbol.as_str(),
                s.value,
                s.scope.docs.as_deref(),
                &s.scope.notes,
            )
        })
        .collect();
    header.push_str(&open_type(
        service,
        p,
        &scope_type(p),
        "scope",
        &scope_values,
    ));
    header.push_str(&format!("\n/** One permission a role grants. Every view has static storage and is NUL-terminated; never free one. */\ntypedef struct {{\n    /** The resource type a grant names. */\n    {p}_string resource_type;\n    /** What grants store. */\n    {p}_string permission;\n}} {p}_role_permission;\n\n/** One role in the registry. Every view and array has static storage; never free one. An empty list has a count of 0 and a NULL array. */\ntypedef struct {{\n    /** What bindings store. */\n    {p}_string value;\n    /** The model's documentation for it, empty when it has none. */\n    {p}_string description;\n    size_t scope_count;\n    /** The scope values it may be bound at, in catalogue order. */\n    const {p}_string *scopes;\n    size_t holder_count;\n    /** The kinds of principal that may hold it; none: any. */\n    const {p}_string *holders;\n    size_t assigned_by_count;\n    /** The roles whose holders alone may assign it; none: whoever may write bindings. */\n    const {p}_string *assigned_by;\n    size_t permission_count;\n    /** What it grants: the permissions this client knows. */\n    const {p}_role_permission *permissions;\n}} {p}_role_description;\n"));
    let mut source = format!(
        "#include \"{}/roles.h\"\n#include <stddef.h>\n\n",
        options.package_name
    );
    let mut registry = String::new();
    for (index, role) in roles.iter().enumerate() {
        registry.push_str(&entry(&mut source, p, index, role));
    }
    let count = roles.len();
    let custom = if custom(service) {
        " The service also accepts custom roles, which are not listed."
    } else {
        ""
    };
    header.push_str(&format!("\n/** Every role, in the service's catalogue order: what a role picker offers; static storage, never freed.{custom} */\nextern const {p}_role_description {p}_roles[{count}];\n/** The number of roles in {p}_roles. */\nextern const size_t {p}_roles_count;\n\n#ifdef __cplusplus\n}}\n#endif\n#endif\n"));
    source.push_str(&format!(
        "const {p}_role_description {p}_roles[{count}] = {{\n{registry}}};\nconst size_t {p}_roles_count = {count};\n"
    ));
    vec![
        File {
            path: format!("include/{}/roles.h", options.package_name),
            contents: header,
        },
        File {
            path: "src/roles.c".into(),
            contents: source,
        },
    ]
}
