//! The client's `pub mod roles { .. }` (docs/roles.md §7 "Clients"): the
//! service's role and scope values as open enums, `Role` and `Scope`, that
//! keep a value they do not know (a newer server's, a custom role's) and
//! write it back unchanged, and `ROLES`, the registry a console's role
//! picker reads. Generated from `crucible_codegen_common::roles`' one rule,
//! as every client's is: public roles and scopes only, in catalogue order,
//! and only public permissions in a role's `permissions`.

use crucible_codegen_common::permissions::code_span;
use crucible_codegen_common::roles::{ClientRole, client_roles, client_scopes};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{DocNotes, Service};

use crate::names::{documented_text, literal, variant};

const RT: &str = "::crucible_server_runtime_rust::__generated";

pub(crate) fn module(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let roles = client_roles(service);
    if roles.is_empty() {
        return String::new();
    }
    let scopes = client_scopes(service);
    crate::names::check_distinct(
        &service.id,
        roles.iter().map(|r| r.role.symbol.as_str()),
        variant,
        "role variant",
        diagnostics,
    );
    crate::names::check_distinct(
        &service.id,
        scopes.iter().map(|s| s.scope.symbol.as_str()),
        variant,
        "scope variant",
        diagnostics,
    );
    let mut out = enumeration(
        "Role",
        "role",
        roles
            .iter()
            .map(|r| {
                (
                    r.role.symbol.as_str(),
                    r.value,
                    r.role.docs.as_deref(),
                    &r.role.notes,
                )
            })
            .collect(),
    );
    out.push_str(&enumeration(
        "Scope",
        "scope",
        scopes
            .iter()
            .map(|s| {
                (
                    s.scope.symbol.as_str(),
                    s.value,
                    s.scope.docs.as_deref(),
                    &s.scope.notes,
                )
            })
            .collect(),
    ));
    let custom = if crucible_codegen_common::roles::custom(service) {
        "\n/// The service also accepts custom roles, which are not listed."
    } else {
        ""
    };
    out.push_str(&format!(
        "\n/// Every role, in the service's catalogue order: what a role picker\n/// offers.{custom}\npub static ROLES: &[{RT}::roles::RoleDescription] = &[\n    {},\n];\n",
        roles.iter().map(entry).collect::<Vec<_>>().join(",\n    ")
    ));
    format!(
        "\n/// The model's roles and the scopes they bind at, as the values bindings\n/// store.\n#[allow(deprecated, dead_code)]\npub mod roles {{\n{}}}\n",
        crate::indent(&out)
    )
}

/// One open enum: `Role` or `Scope`.
fn enumeration(
    name: &str,
    what: &str,
    values: Vec<(&str, &str, Option<&str>, &DocNotes)>,
) -> String {
    let unknown = crate::open::unknown_variant(values.iter().map(|(symbol, ..)| *symbol));
    let mut out = format!(
        "\n/// The service's {what}s. Open: a value this client does not know is\n/// kept as `{unknown}` and written back unchanged. Ordered as the service\n/// lists them (`{unknown}` last), not by value.\n#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]\n{}pub enum {name} {{\n",
        crate::NON_EXHAUSTIVE,
    );
    for (symbol, value, docs, notes) in &values {
        let mut text = docs
            .map(crucible_codegen_common::docs::escape)
            .unwrap_or_default();
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&format!("{}.", code_span(value)));
        documented_text(&mut out, "    ", Some(&text), notes);
        out.push_str(&format!("    {},\n", variant(symbol)));
    }
    let all = values
        .iter()
        .map(|(symbol, ..)| format!("Self::{}", variant(symbol)))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "    /// A value this client does not know.\n    {unknown}(::std::string::String),\n}}\n\nimpl {name} {{\n    /// Every value this client knows, in catalogue order.\n    pub const ALL: &'static [Self] = &[{all}];\n\n    /// The value, as bindings store it.\n    pub fn as_str(&self) -> &str {{\n        match self {{\n"
    ));
    for (symbol, value, ..) in &values {
        out.push_str(&format!(
            "            Self::{} => {},\n",
            variant(symbol),
            literal(value)
        ));
    }
    out.push_str(&format!(
        "            Self::{unknown}(value) => value,\n        }}\n    }}\n}}\n\nimpl {RT}::open::Enum for {name} {{\n    fn as_str(&self) -> &str {{\n        self.as_str()\n    }}\n\n    fn from_string(value: ::std::string::String) -> Self {{\n        match value.as_str() {{\n"
    ));
    for (symbol, value, ..) in &values {
        out.push_str(&format!(
            "            {} => Self::{},\n",
            literal(value),
            variant(symbol)
        ));
    }
    out.push_str(&format!(
        "            _ => Self::{unknown}(value),\n        }}\n    }}\n}}\n\nimpl ::std::convert::From<&str> for {name} {{\n    fn from(value: &str) -> Self {{\n        <Self as {RT}::open::Enum>::from_string(value.to_owned())\n    }}\n}}\n\nimpl ::std::convert::From<::std::string::String> for {name} {{\n    fn from(value: ::std::string::String) -> Self {{\n        <Self as {RT}::open::Enum>::from_string(value)\n    }}\n}}\n\nimpl ::std::fmt::Display for {name} {{\n    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        f.write_str(self.as_str())\n    }}\n}}\n\nimpl {RT}::serde::Serialize for {name} {{\n    fn serialize<S: {RT}::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {{\n        {RT}::open::serialize_enum(self, serializer)\n    }}\n}}\n\nimpl<'de> {RT}::serde::Deserialize<'de> for {name} {{\n    fn deserialize<D: {RT}::serde::Deserializer<'de>>(deserializer: D) -> ::std::result::Result<Self, D::Error> {{\n        {RT}::open::deserialize_enum(deserializer)\n    }}\n}}\n"
    ));
    out
}

/// `&["a", "b"]`.
fn strings<S: AsRef<str>>(items: &[S]) -> String {
    format!(
        "&[{}]",
        items
            .iter()
            .map(|s| literal(s.as_ref()))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The role's `ROLES` entry.
fn entry(role: &ClientRole<'_>) -> String {
    let permissions: Vec<String> = role
        .permissions
        .iter()
        .map(|p| {
            format!(
                "{RT}::roles::RolePermission {{ resource_type: {}, permission: {} }}",
                literal(p.resource_type),
                literal(p.value)
            )
        })
        .collect();
    format!(
        "{RT}::roles::RoleDescription {{\n        value: {},\n        description: {},\n        scopes: {},\n        holders: {},\n        assigned_by: {},\n        permissions: &[{}],\n    }}",
        literal(role.value),
        literal(role.description),
        strings(&role.scopes),
        strings(&role.holders),
        strings(&role.assigned_by),
        permissions.join(", ")
    )
}
