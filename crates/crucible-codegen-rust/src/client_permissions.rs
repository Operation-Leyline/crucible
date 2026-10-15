//! The client's `pub mod permissions { .. }` (docs/permissions.md §6.3):
//! per resource an open enum that keeps values it does not know (a newer
//! server's, an internal one's) and writes them back unchanged, with
//! deprecated aliases as variants `canonical()` maps to their primaries,
//! and `PERMISSIONS`, the registry a console's grant picker reads.
//! Internal permissions never appear, whatever the audience.

use crucible_codegen_common::permissions::{
    ClientResource, client_resources, code_span, notes, primary,
};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{Permission, ResourcePermissions, Service};

use crate::names::{documented_text, literal, variant};

const RT: &str = "::crucible_server_runtime_rust::__generated";

/// The runtime's `Tier` variant for a registry entry's tier name.
fn tier(name: &str) -> &'static str {
    match name {
        "permissionOnly" => "PermissionOnly",
        "notGrantable" => "NotGrantable",
        "public" => "Public",
        other => unreachable!("a client registry has no `{other}` tier"),
    }
}

pub(crate) fn module(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let resources = client_resources(service);
    if resources.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut registry = Vec::new();
    for client in &resources {
        crate::names::check_distinct(
            client.id,
            client.permissions.iter().map(|p| p.symbol.as_str()),
            variant,
            "variant",
            diagnostics,
        );
        out.push_str(&enumeration(
            service.name_of(client.id),
            client.resource,
            &client.permissions,
        ));
        registry.push(entry(client));
    }
    out.push_str(&format!(
        "\n/// Every permission this client knows, by resource type and value,\n/// deprecated aliases included. A grant picker offers only the\n/// `grantable` ones that are not `deprecated`.\npub static PERMISSIONS: &[{RT}::permissions::ResourcePermissions] = &[\n    {},\n];\n",
        registry.join(",\n    ")
    ));
    format!(
        "\n/// The model's permissions, as the values grants store.\n#[allow(deprecated, dead_code)]\npub mod permissions {{\n{}}}\n",
        crate::indent(&out)
    )
}

fn enumeration(shape: &str, resource: &ResourcePermissions, permissions: &[&Permission]) -> String {
    let name = format!("{shape}Permission");
    let unknown = crate::open::unknown_variant(permissions.iter().map(|p| p.symbol.as_str()));
    let mut out = format!(
        "\n/// The permissions `{shape}` accepts (`{}`). Open: a value this client\n/// does not know is kept as `{unknown}` and written back unchanged.\n/// Ordered by declaration (`{unknown}` last), not by value.\n#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]\n{}pub enum {name} {{\n",
        resource.name,
        crate::NON_EXHAUSTIVE,
    );
    for permission in permissions {
        let mut docs = permission
            .docs
            .as_deref()
            .map(crucible_codegen_common::docs::escape)
            .unwrap_or_default();
        if !docs.is_empty() {
            docs.push_str("\n\n");
        }
        let value = code_span(&permission.value);
        if permission.alias_of.is_some() {
            let primary = primary(resource, permission);
            docs.push_str(&format!(
                "{value}, an alias of {}: [`canonical()`](Self::canonical) maps it to [`Self::{}`].",
                code_span(&primary.value),
                variant(&primary.symbol)
            ));
        } else {
            docs.push_str(&format!("{value}."));
        }
        documented_text(&mut out, "    ", Some(&docs), &notes(permission));
        out.push_str(&format!("    {},\n", variant(&permission.symbol)));
    }
    let all = permissions
        .iter()
        .map(|p| format!("Self::{}", variant(&p.symbol)))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "    /// A value this client does not know.\n    {unknown}(::std::string::String),\n}}\n\nimpl {name} {{\n    /// Every value this client knows, deprecated aliases included.\n    pub const ALL: &'static [Self] = &[{all}];\n\n    /// The value, as grants store it.\n    pub fn as_str(&self) -> &str {{\n        match self {{\n"
    ));
    for permission in permissions {
        out.push_str(&format!(
            "            Self::{} => {},\n",
            variant(&permission.symbol),
            literal(&permission.value)
        ));
    }
    out.push_str(&format!(
        "            Self::{unknown}(value) => value,\n        }}\n    }}\n\n    /// The permission a deprecated alias stands for; any other value is\n    /// itself.\n    pub fn canonical(&self) -> Self {{\n"
    ));
    let aliases: Vec<&&Permission> = permissions
        .iter()
        .filter(|p| p.alias_of.is_some())
        .collect();
    if aliases.is_empty() {
        out.push_str("        self.clone()\n");
    } else {
        out.push_str("        match self {\n");
        for alias in aliases {
            out.push_str(&format!(
                "            Self::{} => Self::{},\n",
                variant(&alias.symbol),
                variant(&primary(resource, alias).symbol)
            ));
        }
        out.push_str("            other => other.clone(),\n        }\n");
    }
    out.push_str(&format!(
        "    }}\n}}\n\nimpl {RT}::open::Enum for {name} {{\n    fn as_str(&self) -> &str {{\n        self.as_str()\n    }}\n\n    fn from_string(value: ::std::string::String) -> Self {{\n        match value.as_str() {{\n"
    ));
    for permission in permissions {
        out.push_str(&format!(
            "            {} => Self::{},\n",
            literal(&permission.value),
            variant(&permission.symbol)
        ));
    }
    out.push_str(&format!(
        "            _ => Self::{unknown}(value),\n        }}\n    }}\n}}\n\nimpl ::std::convert::From<&str> for {name} {{\n    fn from(value: &str) -> Self {{\n        <Self as {RT}::open::Enum>::from_string(value.to_owned())\n    }}\n}}\n\nimpl ::std::convert::From<::std::string::String> for {name} {{\n    fn from(value: ::std::string::String) -> Self {{\n        <Self as {RT}::open::Enum>::from_string(value)\n    }}\n}}\n\nimpl ::std::fmt::Display for {name} {{\n    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        f.write_str(self.as_str())\n    }}\n}}\n\nimpl {RT}::serde::Serialize for {name} {{\n    fn serialize<S: {RT}::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {{\n        {RT}::open::serialize_enum(self, serializer)\n    }}\n}}\n\nimpl<'de> {RT}::serde::Deserialize<'de> for {name} {{\n    fn deserialize<D: {RT}::serde::Deserializer<'de>>(deserializer: D) -> ::std::result::Result<Self, D::Error> {{\n        {RT}::open::deserialize_enum(deserializer)\n    }}\n}}\n"
    ));
    out
}

/// The resource's `PERMISSIONS` entry, its permissions sorted by value.
fn entry(client: &ClientResource<'_>) -> String {
    let entries: Vec<String> = client
        .entries()
        .iter()
        .map(|e| {
            let alias_of = match e.alias_of {
                Some(primary) => format!("::std::option::Option::Some({})", literal(primary)),
                None => "::std::option::Option::None".to_owned(),
            };
            format!(
                "{RT}::permissions::PermissionDescription {{ value: {}, description: {}, tier: {RT}::permissions::Tier::{}, grantable: {}, deprecated: {}, alias_of: {alias_of} }}",
                literal(e.value),
                literal(e.description),
                tier(e.tier),
                e.grantable,
                e.deprecated,
            )
        })
        .collect();
    format!(
        "{RT}::permissions::ResourcePermissions {{\n        resource_type: {},\n        permissions: &[\n            {},\n        ],\n    }}",
        literal(&client.resource.name),
        entries.join(",\n            ")
    )
}
