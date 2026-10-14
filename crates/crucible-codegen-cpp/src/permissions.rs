//! `permissions.hpp` (docs/permissions.md §6.3), header-only: in the
//! namespace `<ns>::permissions`, apart from the models, per resource an
//! open string type, `GraphPermission`, shaped like a model enum (a
//! `known_` factory per permission, any other string kept as it is) with
//! `canonical()` mapping a deprecated alias to its primary, and `registry`,
//! the constexpr table a console's grant picker reads. Internal
//! permissions never appear, whatever the audience.

use crate::names::{field, identifier, literal};
use crate::{File, Options};
use crucible_codegen_common::permissions::{
    client_resources, code_span, constant_symbol, notes, primary,
};
use crucible_smithy::Diagnostics;
use crucible_smithy::ir::Service;
use std::collections::BTreeSet;

/// A permission's factory, `known_read`, as a model enum names a value's.
fn factory(symbol: &str) -> String {
    format!("known_{}", field(symbol))
}

/// A literal a `std::string_view` holds whole, a NUL included.
fn view(value: &str) -> String {
    let quoted = literal(value);
    match quoted.strip_prefix("std::string(") {
        Some(rest) => format!("std::string_view({rest}"),
        None => quoted,
    }
}

/// Two permissions of one resource whose factories collide after C++
/// naming, or one with no valid factory: collected, never renamed.
pub fn diagnose(service: &Service, diagnostics: &mut Diagnostics) {
    for client in client_resources(service) {
        let shape = service.name_of(client.id);
        let mut seen = BTreeSet::new();
        for permission in &client.permissions {
            let name = factory(&constant_symbol(&permission.symbol));
            if !identifier(&name) {
                diagnostics.push(
                    client.id,
                    format!(
                        "permission `{}` of `{shape}` has no valid C++ factory: `{name}` is not an identifier",
                        permission.symbol
                    ),
                );
            } else if !seen.insert(name.clone()) {
                diagnostics.push(
                    client.id,
                    format!(
                        "permission `{}` of `{shape}` shares the C++ factory `{shape}Permission::{name}` with another",
                        permission.symbol
                    ),
                );
            }
        }
    }
}

pub fn emit(service: &Service, options: &Options) -> Option<File> {
    let resources = client_resources(service);
    if resources.is_empty() {
        return None;
    }
    let ns = &options.namespace;
    let mut out = format!(
        "#pragma once\n#include <array>\n#include <compare>\n#include <optional>\n#include <span>\n#include <string>\n#include <string_view>\n#include <utility>\n\nnamespace {ns}::permissions {{\n\n/** One permission in the registry. */\nstruct PermissionDescription {{\n    /** What grants store. */\n    std::string_view value;\n    /** The model's documentation for it, empty when it has none. */\n    std::string_view description;\n    /** \"public\", \"permissionOnly\" or \"notGrantable\"; a string, since a newer server may add tiers. */\n    std::string_view tier;\n    /** False for a bundle the service confers whole: never offer it in a picker. */\n    bool grantable;\n    bool deprecated;\n    /** For a deprecated alias, the value it stands for. */\n    std::optional<std::string_view> alias_of;\n}};\n\n/** One resource type's permissions, sorted by value. */\nstruct ResourcePermissions {{\n    std::string_view resource_type;\n    std::span<const PermissionDescription> permissions;\n}};\n"
    );
    let mut entries = String::new();
    let mut registry = String::new();
    for client in &resources {
        let shape = service.name_of(client.id);
        let name = format!("{shape}Permission");
        out.push('\n');
        out.push_str(&crucible_codegen_common::doxygen(
            &format!(
                "The permissions `{shape}` accepts (`{}`), by the values grants store: a known one, or any string a newer server writes, kept as it is.",
                client.resource.name
            ),
            "",
        ));
        out.push_str(&format!("struct {name} {{\n    std::string value;\n    explicit {name}(std::string value = {{}}) : value(std::move(value)) {{}}\n    auto operator<=>(const {name}&) const = default;\n"));
        for permission in &client.permissions {
            // Model text: `doxygen_for` escapes it like every other model
            // comment, so `*/` closes nothing and `@param` is no command.
            let mut docs = permission.docs.clone().unwrap_or_default();
            if permission.alias_of.is_some() {
                if !docs.trim().is_empty() {
                    docs.push_str("\n\n");
                }
                let primary = &primary(client.resource, permission).value;
                docs.push_str(&format!("An alias of {}.", code_span(primary)));
            }
            out.push_str(&crucible_codegen_common::doxygen_for(
                Some(docs.as_str()).filter(|d| !d.trim().is_empty()),
                &notes(permission),
                "    ",
            ));
            out.push_str(&format!(
                "    static {name} {}() {{ return {name}{{{}}}; }}\n",
                factory(&constant_symbol(&permission.symbol)),
                literal(&permission.value)
            ));
        }
        out.push_str(&format!("    /** Returns the permission this value stands for: a deprecated alias's primary, or this value itself, unknown values included. */\n    {name} canonical() const {{\n"));
        for (alias, primary) in client.aliases() {
            out.push_str(&format!(
                "        if (value == {}) return {name}{{{}}};\n",
                view(&alias.value),
                literal(&primary.value)
            ));
        }
        out.push_str("        return *this;\n    }\n};\n");
        entries.push_str(&format!(
            "inline constexpr PermissionDescription {shape}Entries[] = {{\n"
        ));
        for entry in client.entries() {
            entries.push_str(&format!(
                "    {{{}, {}, {}, {}, {}, {}}},\n",
                view(entry.value),
                view(entry.description),
                view(entry.tier),
                entry.grantable,
                entry.deprecated,
                entry
                    .alias_of
                    .map_or_else(|| "std::nullopt".to_owned(), view)
            ));
        }
        entries.push_str("};\n");
        registry.push_str(&format!(
            "    {{{}, detail::{shape}Entries}},\n",
            view(&client.resource.name)
        ));
    }
    out.push_str(&format!("\nnamespace detail {{\n{entries}}}\n\n/** Every permission this client knows, by resource type, deprecated aliases included. A grant picker offers only the grantable ones that are not deprecated. */\ninline constexpr std::array<ResourcePermissions, {}> registry{{{{\n{registry}}}}};\n\n}}\n", resources.len()));
    Some(File {
        path: format!("include/{}/permissions.hpp", options.package_name),
        contents: out,
    })
}
