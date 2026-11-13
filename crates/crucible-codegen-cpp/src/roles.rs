//! `roles.hpp` (docs/roles.md §7 "Clients"), header-only: in the
//! namespace `<ns>::roles`, apart from the models (so never a model's
//! `Role`), the service's role and scope values as open string types,
//! `Role` and `Scope`, shaped like a model enum (a `known_` factory per
//! value, any other string kept as it is), and `registry`, the constexpr
//! table a console's role picker reads. Generated from
//! `crucible_codegen_common::roles`' one rule, as every client's is: public
//! roles and scopes only, in catalogue order, and only public permissions
//! in a role's `permissions`.

use crate::names::{field, identifier, literal};
use crate::{File, Options};
use crucible_codegen_common::permissions::{code_span, constant_symbol};
use crucible_codegen_common::roles::{ClientRole, client_roles, client_scopes, custom};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{DocNotes, Service};
use std::collections::BTreeSet;

/// A value's factory, `known_owner`, as a model enum names a value's.
fn factory(symbol: &str) -> String {
    format!("known_{}", field(&constant_symbol(symbol)))
}

/// A literal a `std::string_view` holds whole, a NUL included.
fn view(value: &str) -> String {
    let quoted = literal(value);
    match quoted.strip_prefix("std::string(") {
        Some(rest) => format!("std::string_view({rest}"),
        None => quoted,
    }
}

/// Two roles (or scopes) whose factories collide after C++ naming, or one
/// with no valid factory: collected, never renamed.
pub fn diagnose(service: &Service, diagnostics: &mut Diagnostics) {
    let roles = client_roles(service);
    if roles.is_empty() {
        return;
    }
    let scopes = client_scopes(service);
    for (what, kind, symbols) in [
        (
            "role",
            "Role",
            roles
                .iter()
                .map(|r| r.role.symbol.as_str())
                .collect::<Vec<_>>(),
        ),
        (
            "scope",
            "Scope",
            scopes.iter().map(|s| s.scope.symbol.as_str()).collect(),
        ),
    ] {
        let mut seen = BTreeSet::new();
        for symbol in symbols {
            let name = factory(symbol);
            if !identifier(&name) {
                diagnostics.push(
                    &service.id,
                    format!(
                        "{what} `{symbol}` has no valid C++ factory: `{name}` is not an identifier"
                    ),
                );
            } else if !seen.insert(name.clone()) {
                diagnostics.push(
                    &service.id,
                    format!(
                        "{what} `{symbol}` shares the C++ factory `roles::{kind}::{name}` with another"
                    ),
                );
            }
        }
    }
}

/// One open string type: `Role` or `Scope`.
fn open_type(
    service: &Service,
    name: &str,
    what: &str,
    values: &[(&str, &str, Option<&str>, &DocNotes)],
) -> String {
    let mut out = format!(
        "\n{}",
        crucible_codegen_common::doxygen(
            &format!(
                "The {what} values of `{}`, by the values bindings store: a known one, or any string a newer server writes, kept as it is.",
                service.name_of(&service.id)
            ),
            "",
        )
    );
    out.push_str(&format!("struct {name} {{\n    std::string value;\n    explicit {name}(std::string value = {{}}) : value(std::move(value)) {{}}\n    auto operator<=>(const {name}&) const = default;\n"));
    for (symbol, value, docs, notes) in values {
        // Model text: `doxygen_for` escapes it like every other model
        // comment, so `*/` closes nothing and `@param` is no command.
        let mut text = docs.map(str::to_owned).unwrap_or_default();
        if !text.trim().is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&format!("{}.", code_span(value)));
        out.push_str(&crucible_codegen_common::doxygen_for(
            crate::links::render(service, Some(text.as_str())).as_deref(),
            notes,
            "    ",
        ));
        out.push_str(&format!(
            "    static {name} {}() {{ return {name}{{{}}}; }}\n",
            factory(symbol),
            literal(value)
        ));
    }
    out.push_str("};\n");
    out
}

/// A `std::span` over a detail array, or `{}` when empty.
fn span(details: &mut String, name: &str, kind: &str, items: &[String]) -> String {
    if items.is_empty() {
        return "{}".to_owned();
    }
    details.push_str(&format!(
        "inline constexpr {kind} {name}[] = {{{}}};\n",
        items.join(", ")
    ));
    format!("detail::{name}")
}

fn entry(details: &mut String, index: usize, role: &ClientRole<'_>) -> String {
    let views = |items: &[&str]| items.iter().map(|v| view(v)).collect::<Vec<_>>();
    let scopes = span(
        details,
        &format!("Scopes{index}"),
        "std::string_view",
        &views(&role.scopes),
    );
    let holders: Vec<&str> = role.holders.iter().map(String::as_str).collect();
    let holders = span(
        details,
        &format!("Holders{index}"),
        "std::string_view",
        &views(&holders),
    );
    let assigned_by = span(
        details,
        &format!("AssignedBy{index}"),
        "std::string_view",
        &views(&role.assigned_by),
    );
    let permissions: Vec<String> = role
        .permissions
        .iter()
        .map(|p| {
            format!(
                "RolePermission{{{}, {}}}",
                view(p.resource_type),
                view(p.value)
            )
        })
        .collect();
    let permissions = span(
        details,
        &format!("Permissions{index}"),
        "RolePermission",
        &permissions,
    );
    format!(
        "    {{{}, {}, {scopes}, {holders}, {assigned_by}, {permissions}}},\n",
        view(role.value),
        view(role.description)
    )
}

pub fn emit(service: &Service, options: &Options) -> Option<File> {
    let roles = client_roles(service);
    if roles.is_empty() {
        return None;
    }
    let scopes = client_scopes(service);
    let ns = &options.namespace;
    let mut out = format!(
        "#pragma once\n#include <array>\n#include <compare>\n#include <span>\n#include <string>\n#include <string_view>\n#include <utility>\n\nnamespace {ns}::roles {{\n"
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
    out.push_str(&open_type(service, "Role", "role", &role_values));
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
    out.push_str(&open_type(service, "Scope", "scope", &scope_values));
    out.push_str("\n/** One permission a role grants. */\nstruct RolePermission {\n    /** The resource type a grant names. */\n    std::string_view resource_type;\n    /** What grants store. */\n    std::string_view permission;\n};\n\n/** One role in the registry. */\nstruct RoleDescription {\n    /** What bindings store. */\n    std::string_view value;\n    /** The model's documentation for it, empty when it has none. */\n    std::string_view description;\n    /** The scope values it may be bound at, in catalogue order. */\n    std::span<const std::string_view> scopes;\n    /** The kinds of principal that may hold it; empty: any. */\n    std::span<const std::string_view> holders;\n    /** The roles whose holders alone may assign it; empty: whoever may write bindings. */\n    std::span<const std::string_view> assigned_by;\n    /** What it grants: the permissions this client knows. */\n    std::span<const RolePermission> permissions;\n};\n");
    let mut details = String::new();
    let mut registry = String::new();
    for (index, role) in roles.iter().enumerate() {
        registry.push_str(&entry(&mut details, index, role));
    }
    let custom = if custom(service) {
        " The service also accepts custom roles, which are not listed."
    } else {
        ""
    };
    out.push_str(&format!("\nnamespace detail {{\n{details}}}\n\n/** Every role, in the service's catalogue order: what a role picker offers.{custom} */\ninline constexpr std::array<RoleDescription, {}> registry{{{{\n{registry}}}}};\n\n}}\n", roles.len()));
    Some(File {
        path: format!("include/{}/roles.hpp", options.package_name),
        contents: out,
    })
}
