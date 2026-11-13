//! The `Roles` namespace (docs/roles.md §7 "Clients"): the service's role
//! and scope values as open classes, `Role::OWNER` and `Scope::ALL_NOTES`,
//! shaped like the permission classes, so a value this client does not
//! know (a newer server's, a custom role's) is kept verbatim; and
//! `Roles\ROLES`, the registry a console's role picker reads. Generated
//! from `crucible_codegen_common::roles`' one rule, as every client's is:
//! public roles and scopes only, in catalogue order, and only public
//! permissions in a role's `permissions`.

use std::collections::BTreeSet;

use crucible_codegen_common::docs;
use crucible_codegen_common::permissions::constant_symbol;
use crucible_codegen_common::roles::{ClientRole, client_roles, client_scopes, custom};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{DocNotes, Service};

use crate::File;
use crate::models::docblock;
use crate::names::{literal, screaming, string};

/// The runtime classes only a client with roles ships: the registry's
/// record types, generic names that belong to the runtime.
pub(crate) const RUNTIME: [(&str, &str); 2] = [
    (
        "RoleDescription",
        include_str!("../runtime/RoleDescription.php"),
    ),
    (
        "RolePermission",
        include_str!("../runtime/RolePermission.php"),
    ),
];

/// The file declaring `ROLES`: a namespace constant, which PSR-4 cannot
/// autoload, so Composer loads it through `autoload.files`.
pub(crate) const REGISTRY: &str = "src/Roles/registry.php";

/// Whether the client gets a `Roles` namespace: the service declares a
/// public role.
pub(crate) fn present(service: &Service) -> bool {
    !client_roles(service).is_empty()
}

/// Two roles (or scopes) that PHP names alike would be one constant.
pub(crate) fn check_names(service: &Service, diagnostics: &mut Diagnostics) {
    let roles = client_roles(service);
    let scopes = client_scopes(service);
    for (what, symbols) in [
        (
            "role",
            roles
                .iter()
                .map(|r| r.role.symbol.as_str())
                .collect::<Vec<_>>(),
        ),
        (
            "scope",
            scopes.iter().map(|s| s.scope.symbol.as_str()).collect(),
        ),
    ] {
        let mut seen = BTreeSet::new();
        for symbol in symbols {
            let name = screaming(&constant_symbol(symbol));
            if !seen.insert(name.clone()) {
                diagnostics.push(
                    &service.id,
                    format!("{what} `{symbol}` shares the PHP constant `{name}` with another"),
                );
            }
        }
    }
}

/// One open class: `Role` or `Scope`.
fn class(
    service: &Service,
    namespace: &str,
    header: &str,
    name: &str,
    what: &str,
    values: &[(&str, &str, Option<&str>, &DocNotes)],
) -> File {
    let mut out = header.to_owned();
    out.push_str(&docblock(
        service,
        namespace,
        Some(&format!(
            "The service's {what}s, by the values bindings store, in catalogue order."
        )),
        &DocNotes::default(),
        "",
    ));
    out.push_str(&format!("final readonly class {name}\n{{\n    /** @param string $value the value bindings store; a value this client does not list is kept as is */\n    public function __construct(public string $value) {{}}\n"));
    for (symbol, value, text, notes) in values {
        // Escaped and deprecated as the model's enum values are.
        out.push_str(&docblock(
            service,
            namespace,
            text.filter(|t| !t.trim().is_empty()),
            notes,
            "    ",
        ));
        out.push_str(&format!(
            "    public const {} = {};\n",
            screaming(&constant_symbol(symbol)),
            literal(value)
        ));
    }
    out.push_str(&format!(
        "    public function __toString(): string {{ return '{name}(' . $this->value . ')'; }}\n}}\n"
    ));
    File {
        path: format!("src/Roles/{name}.php"),
        contents: out,
    }
}

/// `['a', 'b']`.
fn strings<S: AsRef<str>>(items: &[S]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|s| literal(s.as_ref()))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn entry(runtime: &str, role: &ClientRole<'_>) -> String {
    let permissions: Vec<String> = role
        .permissions
        .iter()
        .map(|p| {
            format!(
                "new {runtime}\\RolePermission(resourceType: {}, permission: {})",
                literal(p.resource_type),
                literal(p.value)
            )
        })
        .collect();
    format!(
        "    new {runtime}\\RoleDescription(\n        value: {},\n        description: {},\n        scopes: {},\n        holders: {},\n        assignedBy: {},\n        permissions: [{}],\n    ),\n",
        literal(role.value),
        string(role.description),
        strings(&role.scopes),
        strings(&role.holders),
        strings(&role.assigned_by),
        permissions.join(", ")
    )
}

pub(crate) fn emit(service: &Service, namespace: &str) -> Vec<File> {
    let runtime = format!("\\{namespace}\\Runtime");
    let header = format!("<?php\ndeclare(strict_types=1);\nnamespace {namespace}\\Roles;\n\n");
    let roles = client_roles(service);
    let scopes = client_scopes(service);
    let mut files = vec![
        class(
            service,
            namespace,
            &header,
            "Role",
            "role",
            &roles
                .iter()
                .map(|r| {
                    (
                        r.role.symbol.as_str(),
                        r.value,
                        r.role.docs.as_deref(),
                        &r.role.notes,
                    )
                })
                .collect::<Vec<_>>(),
        ),
        class(
            service,
            namespace,
            &header,
            "Scope",
            "scope",
            &scopes
                .iter()
                .map(|s| {
                    (
                        s.scope.symbol.as_str(),
                        s.value,
                        s.scope.docs.as_deref(),
                        &s.scope.notes,
                    )
                })
                .collect::<Vec<_>>(),
        ),
    ];
    let mut out = header;
    out.push_str(&docs::block_comment(
        "",
        if custom(service) {
            "Every predefined role, in the service's catalogue order: what a role picker offers. The service also accepts custom roles, which are not listed."
        } else {
            "Every role, in the service's catalogue order: what a role picker offers."
        },
        &[format!("@var list<{runtime}\\RoleDescription>")],
    ));
    out.push_str(&format!(
        "const ROLES = [\n{}];\n",
        roles
            .iter()
            .map(|role| entry(&runtime, role))
            .collect::<String>()
    ));
    files.push(File {
        path: REGISTRY.into(),
        contents: out,
    });
    files
}
