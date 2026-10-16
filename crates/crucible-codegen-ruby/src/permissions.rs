//! `permissions.rb` (docs/permissions.md §6.3): in `<Module>::Permissions`,
//! per resource an open class of the values grants store,
//! `GraphPermission::READ`, shaped like the model's string enums, so a value
//! this client does not know is kept verbatim; deprecated aliases with
//! `canonical`; and `PERMISSIONS`, the registry a console's grant picker
//! reads. Internal permissions never appear, whatever the audience.

use std::collections::BTreeSet;

use crucible_codegen_common::docs;
use crucible_codegen_common::permissions::{
    ClientResource, client_resources, code_span, constant_symbol, notes, primary,
};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{Service, Shape};

use crate::models::{enum_constant, yard};
use crate::names::{literal, snake};

/// The runtime files only a client with permissions ships: the registry's
/// record types, generic names that belong to the runtime.
pub(crate) const RUNTIME: [(&str, &str); 2] = [
    (
        "permission_description",
        include_str!("../runtime/permission_description.rb"),
    ),
    (
        "resource_permissions",
        include_str!("../runtime/resource_permissions.rb"),
    ),
];

/// The resources a client sees, sorted by authorization name, each with
/// its shape's name.
fn resources(service: &Service) -> Vec<(&str, ClientResource<'_>)> {
    client_resources(service)
        .into_iter()
        .map(|client| (service.name_of(client.id), client))
        .collect()
}

/// Whether the client gets a `permissions.rb`.
pub(crate) fn present(service: &Service) -> bool {
    !client_resources(service).is_empty()
}

/// `mailSend` is `MAIL_SEND`: the model's enum values are already written
/// in capitals, which [`enum_constant`] keeps, but a permission's symbol is
/// lower camel case, whose word breaks a plain upcase would lose.
pub(crate) fn constant(symbol: &str) -> String {
    enum_constant(&snake(symbol).to_ascii_uppercase())
}

/// A model named `Permissions` would be the constant the permissions'
/// module takes, and two permissions of one resource that Ruby names alike
/// would be one constant.
pub(crate) fn check_names(service: &Service, diagnostics: &mut Diagnostics) {
    if !present(service) {
        return;
    }
    let models = service.shapes.iter().any(|(id, shape)| {
        matches!(
            shape,
            Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_)
        ) && service.name_of(id) == "Permissions"
    });
    if models {
        diagnostics.push(
            &service.id,
            "a model named `Permissions` takes the name of the Ruby module the client's permissions are generated in",
        );
    }
    for (shape, client) in resources(service) {
        let mut seen = BTreeSet::new();
        for permission in &client.permissions {
            let name = constant(&constant_symbol(&permission.symbol));
            if !seen.insert(name.clone()) {
                diagnostics.push(
                    &service.id,
                    format!(
                        "permission `{}` of `{shape}` shares the Ruby constant `{name}` with another",
                        permission.symbol
                    ),
                );
            }
        }
    }
}

pub(crate) fn emit(service: &Service, module: &str) -> String {
    let runtime = format!("::{module}::Runtime");
    let mut out = format!(
        "# frozen_string_literal: true\nmodule {module}\n{}  module Permissions\n",
        docs::line_comment(
            "  ",
            "#",
            "The permissions grants store, by resource type, and {PERMISSIONS}, the registry a grant picker reads."
        )
    );
    let mut registry = String::new();
    for (shape, client) in resources(service) {
        let resource = client.resource;
        let name = format!("{shape}Permission");
        out.push_str(&docs::line_comment(
            "    ",
            "#",
            &format!(
                "The permissions `{shape}` accepts (`{}`), by the values grants store.",
                resource.name
            ),
        ));
        out.push_str(&format!("    class {name}\n      # @return [String] the value grants store; a value this client does not list is kept as is\n      attr_reader :value\n      def initialize(value)\n        @value = {runtime}::WireCodec.string(value).freeze\n        freeze\n      end\n      def ==(other) = other.is_a?(self.class) && @value == other.value\n      alias eql? ==\n      def hash = @value.hash\n"));
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
            out.push_str(&yard(
                service,
                Some(text.as_str()).filter(|t| !t.trim().is_empty()),
                &notes(permission),
                "      ",
            ));
            out.push_str(&format!(
                "      {} = new({})\n",
                constant(&constant_symbol(&permission.symbol)),
                literal(&permission.value)
            ));
        }
        out.push_str(&docs::line_comment(
            "      ",
            "#",
            &format!(
                "The permission this one stands for: a deprecated alias's primary, or this one, unknown values included.\n\n@return [{name}]"
            ),
        ));
        out.push_str("      def canonical\n");
        for (alias, primary) in client.aliases() {
            out.push_str(&format!(
                "        return {} if self == {}\n",
                constant(&constant_symbol(&primary.symbol)),
                constant(&constant_symbol(&alias.symbol))
            ));
        }
        out.push_str(&format!(
            "        self\n      end\n      def inspect = '{name}(' + @value + ')'\n      alias to_s inspect\n    end\n"
        ));
        let entries: String = client
            .entries()
            .iter()
            .map(|e| {
                let alias = e
                    .alias_of
                    .map(|primary| format!(", alias_of: {}", literal(primary)))
                    .unwrap_or_default();
                format!(
                    "        {runtime}::PermissionDescription.new(value: {}, description: {}, tier: {}, grantable: {}, deprecated: {}{alias}),\n",
                    literal(e.value),
                    literal(e.description),
                    literal(e.tier),
                    e.grantable,
                    e.deprecated,
                )
            })
            .collect();
        registry.push_str(&format!(
            "      {runtime}::ResourcePermissions.new(resource_type: {}, permissions: [\n{entries}      ].freeze),\n",
            literal(&resource.name)
        ));
    }
    out.push_str(&docs::line_comment(
        "    ",
        "#",
        "Every permission this client knows, by resource type, deprecated aliases included. A grant picker offers only the `grantable` ones that are not `deprecated`.\n\n@return [Array<Runtime::ResourcePermissions>]",
    ));
    out.push_str(&format!(
        "    PERMISSIONS = [\n{registry}    ].freeze\n  end\nend\n"
    ));
    out
}
