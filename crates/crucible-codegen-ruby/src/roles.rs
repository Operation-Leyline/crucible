//! `roles.rb` (docs/roles.md §7 "Clients"): in `<Module>::Roles`, the
//! service's role and scope values as open classes, `Role::OWNER` and
//! `Scope::ALL_NOTES`, shaped like the permission classes, so a value this
//! client does not know (a newer server's, a custom role's) is kept
//! verbatim; and `ROLES`, the registry a console's role picker reads.
//! Generated from `crucible_codegen_common::roles`' one rule, as every
//! client's is: public roles and scopes only, in catalogue order, and only
//! public permissions in a role's `permissions`.

use std::collections::BTreeSet;

use crucible_codegen_common::docs;
use crucible_codegen_common::permissions::constant_symbol;
use crucible_codegen_common::roles::{ClientRole, client_roles, client_scopes, custom};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{DocNotes, Service, Shape};

use crate::models::yard;
use crate::names::literal;
use crate::permissions::constant;

/// The runtime files only a client with roles ships: the registry's record
/// types, generic names that belong to the runtime.
pub(crate) const RUNTIME: [(&str, &str); 2] = [
    (
        "role_permission",
        include_str!("../runtime/role_permission.rb"),
    ),
    (
        "role_description",
        include_str!("../runtime/role_description.rb"),
    ),
];

/// Whether the client gets a `roles.rb`: the service declares a public role.
pub(crate) fn present(service: &Service) -> bool {
    !client_roles(service).is_empty()
}

/// A model named `Roles` would be the constant the roles' module takes,
/// and two roles (or scopes) that Ruby names alike would be one constant.
pub(crate) fn check_names(service: &Service, diagnostics: &mut Diagnostics) {
    if !present(service) {
        return;
    }
    let models = service.shapes.iter().any(|(id, shape)| {
        matches!(
            shape,
            Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_)
        ) && service.name_of(id) == "Roles"
    });
    if models {
        diagnostics.push(
            &service.id,
            "a model named `Roles` takes the name of the Ruby module the client's roles are generated in",
        );
    }
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
            let name = constant(&constant_symbol(symbol));
            if !seen.insert(name.clone()) {
                diagnostics.push(
                    &service.id,
                    format!("{what} `{symbol}` shares the Ruby constant `{name}` with another"),
                );
            }
        }
    }
}

/// One open class: `Role` or `Scope`.
fn class(
    service: &Service,
    out: &mut String,
    runtime: &str,
    name: &str,
    what: &str,
    values: &[(&str, &str, Option<&str>, &DocNotes)],
) {
    out.push_str(&docs::line_comment(
        "    ",
        "#",
        &format!("The service's {what}s, by the values bindings store, in catalogue order."),
    ));
    out.push_str(&format!("    class {name}\n      # @return [String] the value bindings store; a value this client does not list is kept as is\n      attr_reader :value\n      def initialize(value)\n        @value = {runtime}::WireCodec.string(value).freeze\n        freeze\n      end\n      def ==(other) = other.is_a?(self.class) && @value == other.value\n      alias eql? ==\n      def hash = @value.hash\n"));
    for (symbol, value, text, notes) in values {
        // Escaped and deprecated as the model's enum values are.
        out.push_str(&yard(
            service,
            text.filter(|t| !t.trim().is_empty()),
            notes,
            "      ",
        ));
        out.push_str(&format!(
            "      {} = new({})\n",
            constant(&constant_symbol(symbol)),
            literal(value)
        ));
    }
    out.push_str(&format!(
        "      def inspect = '{name}(' + @value + ')'\n      alias to_s inspect\n    end\n"
    ));
}

/// `['a', 'b'].freeze`.
fn strings<S: AsRef<str>>(items: &[S]) -> String {
    format!(
        "[{}].freeze",
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
                "{runtime}::RolePermission.new(resource_type: {}, permission: {})",
                literal(p.resource_type),
                literal(p.value)
            )
        })
        .collect();
    format!(
        "      {runtime}::RoleDescription.new(\n        value: {},\n        description: {},\n        scopes: {},\n        holders: {},\n        assigned_by: {},\n        permissions: [{}].freeze,\n      ),\n",
        literal(role.value),
        literal(role.description),
        strings(&role.scopes),
        strings(&role.holders),
        strings(&role.assigned_by),
        permissions.join(", ")
    )
}

pub(crate) fn emit(service: &Service, module: &str) -> String {
    let runtime = format!("::{module}::Runtime");
    let mut out = format!(
        "# frozen_string_literal: true\nmodule {module}\n{}  module Roles\n",
        docs::line_comment(
            "  ",
            "#",
            "The roles and scopes bindings store, and {ROLES}, the registry a role picker reads."
        )
    );
    let roles = client_roles(service);
    let scopes = client_scopes(service);
    class(
        service,
        &mut out,
        &runtime,
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
    );
    class(
        service,
        &mut out,
        &runtime,
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
    );
    out.push_str(&docs::line_comment(
        "    ",
        "#",
        &format!(
            "{}\n\n@return [Array<Runtime::RoleDescription>]",
            if custom(service) {
                "Every predefined role, in the service's catalogue order: what a role picker offers. The service also accepts custom roles, which are not listed."
            } else {
                "Every role, in the service's catalogue order: what a role picker offers."
            }
        ),
    ));
    out.push_str(&format!(
        "    ROLES = [\n{}    ].freeze\n  end\nend\n",
        roles
            .iter()
            .map(|role| entry(&runtime, role))
            .collect::<String>()
    ));
    out
}
