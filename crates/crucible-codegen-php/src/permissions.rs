//! The `Permissions` namespace (docs/permissions.md §6.3): per resource an
//! open class of the values grants store, `GraphPermission::READ`, shaped
//! like the model's string enums, so a value this client does not know is
//! kept verbatim; deprecated aliases with `canonical()`; and
//! `Permissions\PERMISSIONS`, the registry a console's grant picker reads.
//! Internal permissions never appear, whatever the audience.

use std::collections::BTreeSet;

use crucible_codegen_common::docs;
use crucible_codegen_common::permissions::{
    ClientResource, client_resources, code_span, constant_symbol, notes, primary,
};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{DocNotes, Service};

use crate::File;
use crate::models::docblock;
use crate::names::{literal, screaming, string};

/// The runtime classes only a client with permissions ships: the
/// registry's record types, generic names that belong to the runtime.
pub(crate) const RUNTIME: [(&str, &str); 2] = [
    (
        "PermissionDescription",
        include_str!("../runtime/PermissionDescription.php"),
    ),
    (
        "ResourcePermissions",
        include_str!("../runtime/ResourcePermissions.php"),
    ),
];

/// The file declaring `PERMISSIONS`: a namespace constant, which PSR-4
/// cannot autoload, so Composer loads it through `autoload.files`.
pub(crate) const REGISTRY: &str = "src/Permissions/registry.php";

/// The resources a client sees, sorted by authorization name, each with
/// its shape's name.
fn resources(service: &Service) -> Vec<(&str, ClientResource<'_>)> {
    client_resources(service)
        .into_iter()
        .map(|client| (service.name_of(client.id), client))
        .collect()
}

/// Whether the client gets a `Permissions` namespace.
pub(crate) fn present(service: &Service) -> bool {
    !client_resources(service).is_empty()
}

/// Two permissions of one resource that PHP names alike would be one
/// constant, and PHP class names are case-insensitive, so two resources
/// whose names differ only in case would be one class.
pub(crate) fn check_names(service: &Service, diagnostics: &mut Diagnostics) {
    let mut classes = BTreeSet::new();
    for (shape, client) in resources(service) {
        let class = format!("{shape}Permission");
        if !classes.insert(class.to_ascii_lowercase()) {
            diagnostics.push(
                &service.id,
                format!("the PHP permission class `{class}` collides with another"),
            );
        }
        let mut seen = BTreeSet::new();
        for permission in &client.permissions {
            let name = screaming(&constant_symbol(&permission.symbol));
            if !seen.insert(name.clone()) {
                diagnostics.push(
                    &service.id,
                    format!(
                        "permission `{}` of `{shape}` shares the PHP constant `{name}` with another",
                        permission.symbol
                    ),
                );
            }
        }
    }
}

pub(crate) fn emit(service: &Service, namespace: &str) -> Vec<File> {
    let runtime = format!("\\{namespace}\\Runtime");
    let header =
        format!("<?php\ndeclare(strict_types=1);\nnamespace {namespace}\\Permissions;\n\n");
    let mut files = Vec::new();
    let mut registry = String::new();
    for (shape, client) in resources(service) {
        let resource = client.resource;
        let class = format!("{shape}Permission");
        let mut out = header.clone();
        out.push_str(&docblock(
            service,
            namespace,
            Some(&format!(
                "The permissions `{shape}` accepts (`{}`), by the values grants store.",
                resource.name
            )),
            &DocNotes::default(),
            "",
        ));
        out.push_str(&format!("final readonly class {class}\n{{\n    /** @param string $value the value grants store; a value this client does not list is kept as is */\n    public function __construct(public string $value) {{}}\n"));
        for permission in &client.permissions {
            let mut text = permission.docs.clone().unwrap_or_default();
            if permission.alias_of.is_some() {
                if !text.trim().is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str(&format!(
                    "An alias of {}.",
                    code_span(&primary(resource, permission).value)
                ));
            }
            // Escaped and deprecated as the model's enum values are.
            out.push_str(&docblock(
                service,
                namespace,
                Some(text.as_str()).filter(|t| !t.trim().is_empty()),
                &notes(permission),
                "    ",
            ));
            out.push_str(&format!(
                "    public const {} = {};\n",
                screaming(&constant_symbol(&permission.symbol)),
                literal(&permission.value)
            ));
        }
        out.push_str(&docs::block_comment(
            "    ",
            "The permission this one stands for: a deprecated alias's primary, or this one, unknown values included.",
            &[],
        ));
        let aliases = client.aliases();
        if aliases.is_empty() {
            out.push_str(
                "    public function canonical(): self\n    {\n        return $this;\n    }\n",
            );
        } else {
            out.push_str(
                "    public function canonical(): self\n    {\n        return match ($this->value) {\n",
            );
            for (alias, primary) in aliases {
                out.push_str(&format!(
                    "            self::{} => new self(self::{}),\n",
                    screaming(&constant_symbol(&alias.symbol)),
                    screaming(&constant_symbol(&primary.symbol))
                ));
            }
            out.push_str("            default => $this,\n        };\n    }\n");
        }
        out.push_str(&format!(
            "    public function __toString(): string {{ return '{class}(' . $this->value . ')'; }}\n}}\n"
        ));
        files.push(File {
            path: format!("src/Permissions/{class}.php"),
            contents: out,
        });
        let entries: String = client
            .entries()
            .iter()
            .map(|e| {
                let alias = e
                    .alias_of
                    .map(|primary| format!(", aliasOf: {}", literal(primary)))
                    .unwrap_or_default();
                format!(
                    "        new {runtime}\\PermissionDescription(value: {}, description: {}, tier: {}, grantable: {}, deprecated: {}{alias}),\n",
                    literal(e.value),
                    string(e.description),
                    literal(e.tier),
                    e.grantable,
                    e.deprecated,
                )
            })
            .collect();
        registry.push_str(&format!(
            "    new {runtime}\\ResourcePermissions(resourceType: {}, permissions: [\n{entries}    ]),\n",
            literal(&resource.name)
        ));
    }
    let mut out = header;
    out.push_str(&docs::block_comment(
        "",
        "Every permission this client knows, by resource type, deprecated aliases included. A grant picker offers only the `grantable` ones that are not `deprecated`.",
        &[format!("@var list<{runtime}\\ResourcePermissions>")],
    ));
    out.push_str(&format!("const PERMISSIONS = [\n{registry}];\n"));
    files.push(File {
        path: REGISTRY.into(),
        contents: out,
    });
    files
}
