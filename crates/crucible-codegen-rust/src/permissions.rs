//! Typed permissions in the server bindings (docs/permissions.md §6.2): the
//! `'static` descriptions the runtime's `Permissions` interceptor reads,
//! the closed per-resource enums and typed references a service's
//! Authorizer matches on, and per operation its `REQUIREMENTS`, the small
//! `CHECKS` function that binds them to the decoded input, and an
//! `Obligation` constant per `by handler` clause. Everything that decides
//! stays in the runtime.

use std::collections::BTreeMap;

use crucible_codegen_common::permissions::{
    aliases, code_span, denied_message, not_found_message, notes,
};
use crucible_smithy::ir::permissions::Binding;
use crucible_smithy::ir::{
    Condition, Member, Mode, Operation, Permission, Requirements, ResourcePermissions, Service,
    Shape, SimpleKind, Tier, Value,
};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::info::constant;
use crate::names::{doc, documented, field, literal, variant};

const P: &str = "::crucible_server_runtime_rust::__generated::permissions";
const ERROR: &str = "::crucible_server_runtime_rust::__generated::error::Error";

/// The Rust spellings of one resource's generated items.
struct Names<'a> {
    resource: &'a ResourcePermissions,
    /// `Inbox`
    shape: String,
    /// `INBOX`: its `ResourceInfo` static.
    info: String,
}

impl<'a> Names<'a> {
    fn new(service: &Service, id: &ShapeId, resource: &'a ResourcePermissions) -> Self {
        let shape = service.name_of(id).to_owned();
        Self {
            resource,
            info: constant(&shape),
            shape,
        }
    }

    fn permission_enum(&self) -> String {
        format!("{}Permission", self.shape)
    }

    /// Its variant of the `Permission` and `Resource` unions: the shape's
    /// name, except `Self`, which no variant may be called.
    fn union_variant(&self) -> String {
        if self.shape == "Self" {
            "Self_".to_owned()
        } else {
            self.shape.clone()
        }
    }

    fn reference(&self) -> String {
        format!("{}Ref", self.shape)
    }

    /// The `PermissionInfo` static of a primary: `INBOX__MAIL_SEND`. The
    /// double underscore keeps `Inbox.mailSend` and `InboxMail.send` apart.
    fn permission_static(&self, permission: &Permission) -> String {
        format!("{}__{}", self.info, constant(&permission.symbol))
    }

    fn primaries(&self) -> impl Iterator<Item = &'a Permission> {
        self.resource
            .permissions
            .iter()
            .filter(|p| p.alias_of.is_none())
    }
}

fn names(service: &Service) -> Vec<(&ShapeId, Names<'_>)> {
    service
        .permissions
        .iter()
        .map(|(id, resource)| (id, Names::new(service, id, resource)))
        .collect()
}

/// The path of a primary's `PermissionInfo` from the service module.
fn info_path(service: &Service, resource: &ShapeId, symbol: &str) -> Option<String> {
    let permissions = service.permissions.get(resource)?;
    let names = Names::new(service, resource, permissions);
    let permission = permissions.permission(symbol)?;
    let primary = crucible_codegen_common::permissions::primary(permissions, permission);
    Some(format!("permissions::{}", names.permission_static(primary)))
}

/// `pub mod permissions { .. }`, or nothing for a model without
/// permissions.
pub(crate) fn module(service: &Service, diagnostics: &mut Diagnostics) -> String {
    if service.permissions.is_empty() {
        return String::new();
    }
    let resources = names(service);
    let mut taken: BTreeMap<String, ShapeId> = BTreeMap::new();
    let mut take = |name: String, id: &ShapeId, diagnostics: &mut Diagnostics| {
        if let Some(previous) = taken.insert(name.clone(), id.clone()) {
            diagnostics.push(
                id,
                format!("the generated permission item `{name}` collides with one generated for `{previous}`"),
            );
        }
    };
    for name in ["Permission", "Resource"] {
        take(name.to_owned(), &service.id, diagnostics);
    }
    let mut out = String::new();
    for (id, names) in &resources {
        // The unions' variants share no namespace with the module's items.
        take(
            format!("Permission::{}", names.union_variant()),
            id,
            diagnostics,
        );
        take(names.info.clone(), id, diagnostics);
        take(names.permission_enum(), id, diagnostics);
        take(names.reference(), id, diagnostics);
        crate::names::check_distinct(
            id,
            names.resource.permissions.iter().map(|p| p.symbol.as_str()),
            variant,
            "variant",
            diagnostics,
        );
        associated_names(id, names, diagnostics);
        let resource = names.resource;
        out.push_str(&format!(
            "\n/// `{}`, as authorization names it.\npub(super) static {}: {P}::ResourceInfo = {P}::ResourceInfo {{\n    name: {},\n    identifiers: &[{}],\n    not_found: {},\n}};\n",
            names.shape,
            names.info,
            literal(&resource.name),
            resource
                .identifiers
                .iter()
                .map(|identifier| literal(identifier))
                .collect::<Vec<_>>()
                .join(", "),
            literal(&not_found_message(resource)),
        ));
        for permission in names.primaries() {
            take(names.permission_static(permission), id, diagnostics);
            let aliases: Vec<String> = aliases(resource, permission)
                .map(|alias| literal(&alias.value))
                .collect();
            out.push_str(&format!(
                "pub(super) static {}: {P}::PermissionInfo = {P}::PermissionInfo {{\n    resource: &{},\n    symbol: {},\n    value: {},\n    aliases: &[{}],\n    tier: {P}::Tier::{},\n    denied: {},\n}};\n",
                names.permission_static(permission),
                names.info,
                literal(&permission.symbol),
                literal(&permission.value),
                aliases.join(", "),
                tier(permission.tier),
                literal(&denied_message(resource, permission)),
            ));
        }
        out.push_str(&permission_enum(names));
        out.push_str(&reference(names, service.not_found.is_some()));
    }
    out.push_str(&union(&resources));
    out.push_str(&resource_union(&resources));
    format!(
        "\n/// The model's permissions: a closed enum per resource, [`Permission`]\n/// over all of them, and typed references to the instances checks name.\n#[allow(deprecated, unreachable_patterns, clippy::all)]\npub mod permissions {{\n{}}}\n",
        crate::indent(&out)
    )
}

/// The names one resource's enum defines as `Self::..`: its generated
/// constants, a variant per primary and a constant per alias. Two alike
/// would not compile, so they are a diagnostic.
fn associated_names(id: &ShapeId, names: &Names<'_>, diagnostics: &mut Diagnostics) {
    let mut taken: BTreeMap<String, String> = ["ALL", "RESOURCE"]
        .into_iter()
        .map(|name| (name.to_owned(), format!("the generated `{name}`")))
        .collect();
    for permission in &names.resource.permissions {
        let name = if permission.alias_of.is_some() {
            constant(&permission.symbol)
        } else {
            variant(&permission.symbol)
        };
        let this = format!("permission `{}`", permission.symbol);
        if let Some(previous) = taken.insert(name.clone(), this.clone()) {
            diagnostics.push(
                id,
                format!(
                    "{this} generates `{}::{name}`, as {previous} does",
                    names.permission_enum()
                ),
            );
        }
    }
}

fn tier(tier: Tier) -> &'static str {
    match tier {
        Tier::Public => "Public",
        Tier::PermissionOnly => "PermissionOnly",
        Tier::NotGrantable => "NotGrantable",
        Tier::Internal => "Internal",
    }
}

fn permission_enum(names: &Names<'_>) -> String {
    let name = names.permission_enum();
    let resource = names.resource;
    let mut out = format!(
        "\n/// The permissions {} `{}` accepts (`{}`). Closed: an Authorizer's\n/// role table over it is an exhaustive `match`.\n#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\npub enum {name} {{\n",
        article(&names.shape),
        names.shape,
        resource.name
    );
    for permission in names.primaries() {
        let mut docs = model_docs(permission);
        docs.push_str(&format!("{}.", code_span(&permission.value)));
        documented(&mut out, "    ", Some(&docs), &permission.notes);
        out.push_str(&format!("    {},\n", variant(&permission.symbol)));
    }
    out.push_str("}\n\n");
    out.push_str(&format!("impl {name} {{\n"));
    out.push_str(&format!(
        "    /// Every permission, aliases excluded, in declaration order.\n    pub const ALL: &'static [Self] = &[{}];\n    /// The resource these permissions are on.\n    pub const RESOURCE: &'static {P}::ResourceInfo = &{};\n",
        names
            .primaries()
            .map(|p| format!("Self::{}", variant(&p.symbol)))
            .collect::<Vec<_>>()
            .join(", "),
        names.info
    ));
    for alias in resource.permissions.iter().filter(|p| p.alias_of.is_some()) {
        let primary = crucible_codegen_common::permissions::primary(resource, alias);
        let mut docs = model_docs(alias);
        docs.push_str(&format!(
            "{}, an alias of {}: parsing it yields [`Self::{}`].",
            code_span(&alias.value),
            code_span(&primary.value),
            variant(&primary.symbol)
        ));
        documented(&mut out, "    ", Some(&docs), &notes(alias));
        out.push_str(&format!(
            "    pub const {}: Self = Self::{};\n",
            constant(&alias.symbol),
            variant(&primary.symbol)
        ));
    }
    out.push_str(&format!(
        "    /// The permission's value: what grants store.\n    pub const fn value(self) -> &'static str {{\n        self.info().value\n    }}\n    /// The permission's description, for the runtime and the Authorizer.\n    pub const fn info(self) -> &'static {P}::PermissionInfo {{\n        match self {{\n"
    ));
    for permission in names.primaries() {
        out.push_str(&format!(
            "            Self::{} => &{},\n",
            variant(&permission.symbol),
            names.permission_static(permission)
        ));
    }
    out.push_str("        }\n    }\n");
    out.push_str("    /// The permission a stored value names, aliases included; `None` for a\n    /// value this model does not declare, such as an old grant's.\n    pub fn parse(value: &str) -> ::std::option::Option<Self> {\n        match value {\n");
    for permission in names.primaries() {
        let values: Vec<String> = std::iter::once(permission)
            .chain(aliases(resource, permission))
            .map(|p| literal(&p.value))
            .collect();
        out.push_str(&format!(
            "            {} => ::std::option::Option::Some(Self::{}),\n",
            values.join(" | "),
            variant(&permission.symbol)
        ));
    }
    out.push_str("            _ => ::std::option::Option::None,\n        }\n    }\n}\n\n");
    out.push_str(&format!(
        "impl ::std::convert::From<{name}> for &'static {P}::PermissionInfo {{\n    fn from(permission: {name}) -> Self {{\n        permission.info()\n    }}\n}}\n\nimpl ::std::fmt::Display for {name} {{\n    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        f.write_str(self.value())\n    }}\n}}\n"
    ));
    out
}

/// A permission's own documentation, escaped as model text, followed by a
/// blank line when there is any.
fn model_docs(permission: &Permission) -> String {
    let mut docs = permission
        .docs
        .as_deref()
        .map(crucible_codegen_common::docs::escape)
        .unwrap_or_default();
    if !docs.trim().is_empty() {
        docs.push_str("\n\n");
    }
    docs
}

fn article(noun: &str) -> &'static str {
    if noun.starts_with(|c: char| "AEIOUaeiou".contains(c)) {
        "an"
    } else {
        "a"
    }
}

/// `InboxRef`: one instance, or every instance under its parents, with the
/// conversions to and from the runtime's `ResourceRef`.
/// `designated`: the model designates its own not-found error, which only
/// the runtime (through the service's factory) can build.
fn reference(names: &Names<'_>, designated: bool) -> String {
    let name = names.reference();
    let identifiers = &names.resource.identifiers;
    let Some((leaf, parents)) = identifiers.split_last() else {
        return String::new();
    };
    let mut out = format!(
        "\n/// One `{}`, or every one under its parents (`{}(*)`): a typed view of a\n/// check's [`ResourceRef`]({P}::ResourceRef).\n#[derive(Debug, Clone, PartialEq, Eq, Hash)]\npub struct {name} {{\n",
        names.shape, names.shape
    );
    for parent in parents {
        out.push_str(&format!(
            "    pub {}: ::std::string::String,\n",
            field(parent)
        ));
    }
    out.push_str(&format!("    pub {}: {P}::Target,\n}}\n\n", field(leaf)));
    let parameters: Vec<String> = parents
        .iter()
        .map(|parent| {
            format!(
                "{}: impl ::std::convert::Into<::std::string::String>",
                field(parent)
            )
        })
        .collect();
    let mut one = parameters.clone();
    one.push(format!(
        "{}: impl ::std::convert::Into<::std::string::String>",
        field(leaf)
    ));
    let assign: Vec<String> = parents
        .iter()
        .map(|parent| format!("{}: {}.into()", field(parent), field(parent)))
        .collect();
    let mut one_fields = assign.clone();
    one_fields.push(format!(
        "{}: {P}::Target::One({}.into())",
        field(leaf),
        field(leaf)
    ));
    let mut all_fields = assign;
    all_fields.push(format!("{}: {P}::Target::All", field(leaf)));
    let targets: Vec<String> = parents
        .iter()
        .map(|parent| format!("{P}::Target::from(&self.{})", field(parent)))
        .chain(std::iter::once(format!("self.{}.clone()", field(leaf))))
        .collect();
    let read: Vec<String> = parents
        .iter()
        .enumerate()
        .map(|(index, parent)| format!("{}: one(&identifiers[{index}])?.to_owned()", field(parent)))
        .chain(std::iter::once(format!(
            "{}: identifiers[{}].clone()",
            field(leaf),
            parents.len()
        )))
        .collect();
    let resource = &names.resource.name;
    let info = &names.info;
    let not_found = if designated {
        "    /// The service's not-found error for it, built by the\n    /// `not_found_factory` that answers a hidden instance, so a genuinely\n    /// missing one reads alike.\n    pub fn not_found(&self) -> ::crucible_server_runtime_rust::__generated::error::Error {\n        ::crucible_server_runtime_rust::__generated::permission_not_found(&self.to_ref())\n    }\n".to_owned()
    } else {
        format!(
            "    /// The framework's 404 for it: what a hidden instance answers, so a\n    /// genuinely missing one reads alike.\n    pub fn not_found(&self) -> {ERROR} {{\n        {info}.not_found_error()\n    }}\n"
        )
    };
    out.push_str(&format!(
        "impl {name} {{\n    /// The resource's description.\n    pub const INFO: &'static {P}::ResourceInfo = &{info};\n    /// One instance.\n    pub fn new({}) -> Self {{\n        Self {{ {} }}\n    }}\n    /// Every instance under the parents, including ones created later.\n    pub fn all({}) -> Self {{\n        Self {{ {} }}\n    }}\n{not_found}    fn to_ref(&self) -> {P}::ResourceRef {{\n        {P}::ResourceRef::new(&{info}, [{}])\n    }}\n}}\n\n",
        one.join(", "),
        one_fields.join(", "),
        parameters.join(", "),
        all_fields.join(", "),
        targets.join(", "),
    ));
    for source in [format!("&{name}"), name.clone()] {
        out.push_str(&format!(
            "impl ::std::convert::From<{source}> for {P}::ResourceRef {{\n    fn from(reference: {source}) -> Self {{\n        reference.to_ref()\n    }}\n}}\n\n"
        ));
    }
    let helper = if parents.is_empty() {
        String::new()
    } else {
        format!(
            "        fn one(target: &{P}::Target) -> ::std::result::Result<&str, {ERROR}> {{\n            target.as_one().ok_or_else(|| {ERROR}::internal(\"a parent identifier is `*`\"))\n        }}\n"
        )
    };
    out.push_str(&format!(
        "/// The check's resource, if it is one `{}` (or all of them under\n/// one parent); otherwise an internal error.\nimpl ::std::convert::TryFrom<&{P}::ResourceRef> for {name} {{\n    type Error = {ERROR};\n    fn try_from(reference: &{P}::ResourceRef) -> ::std::result::Result<Self, Self::Error> {{\n{helper}        let identifiers = reference.identifiers();\n        if reference.resource().name != {} || identifiers.len() != {} {{\n            return ::std::result::Result::Err({ERROR}::internal(::std::format!(\"{{reference}} is not one {resource}\")));\n        }}\n        ::std::result::Result::Ok(Self {{ {} }})\n    }}\n}}\n",
        names.shape,
        literal(resource),
        identifiers.len(),
        read.join(", "),
    ));
    out
}

/// `Permission`: every resource's enum, for a registry, a parsed grant and
/// an Authorizer's exhaustive match.
fn union(resources: &[(&ShapeId, Names<'_>)]) -> String {
    let mut out = String::from(
        "\n/// Any permission of the model, by resource. Variants, and so the\n/// derived order, follow the resources' shape ids; `SERVICE`'s registry\n/// and the clients' `PERMISSIONS` sort by authorization name and value.\n#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\npub enum Permission {\n",
    );
    for (_, names) in resources {
        out.push_str(&format!(
            "    {}({}),\n",
            names.union_variant(),
            names.permission_enum()
        ));
    }
    out.push_str("}\n\nimpl Permission {\n");
    let all: Vec<String> = resources
        .iter()
        .flat_map(|(_, names)| {
            names.primaries().map(move |p| {
                format!(
                    "Self::{}({}::{})",
                    names.union_variant(),
                    names.permission_enum(),
                    variant(&p.symbol)
                )
            })
        })
        .collect();
    out.push_str(&format!(
        "    /// Every permission, aliases excluded: by resource shape id, then in\n    /// declaration order.\n    pub const ALL: &'static [Self] = &[{}];\n",
        all.join(", ")
    ));
    out.push_str("    /// The permission `value` (or an alias of it) on the resource whose\n    /// authorization name is `resource`: what a stored grant names.\n    pub fn parse(resource: &str, value: &str) -> ::std::option::Option<Self> {\n        match resource {\n");
    for (_, names) in resources {
        out.push_str(&format!(
            "            {} => {}::parse(value).map(Self::{}),\n",
            literal(&names.resource.name),
            names.permission_enum(),
            names.union_variant()
        ));
    }
    out.push_str("            _ => ::std::option::Option::None,\n        }\n    }\n");
    out.push_str("    /// The permissions of the resource whose authorization name is\n    /// `resource`, aliases excluded; empty for one the model does not know.\n    pub fn of(resource: &str) -> &'static [Self] {\n        match resource {\n");
    for (_, names) in resources {
        let values: Vec<String> = names
            .primaries()
            .map(|p| {
                format!(
                    "Self::{}({}::{})",
                    names.union_variant(),
                    names.permission_enum(),
                    variant(&p.symbol)
                )
            })
            .collect();
        out.push_str(&format!(
            "            {} => &[{}],\n",
            literal(&names.resource.name),
            values.join(", ")
        ));
    }
    out.push_str("            _ => &[],\n        }\n    }\n");
    out.push_str(&format!(
        "    /// The typed permission a check names.\n    ///\n    /// # Panics\n    ///\n    /// When `info` is not one of this model's permissions: every check the\n    /// runtime hands an Authorizer of this service names one.\n    pub fn from_info(info: &{P}::PermissionInfo) -> Self {{\n        match Self::parse(info.resource.name, info.value) {{\n            ::std::option::Option::Some(permission) => permission,\n            ::std::option::Option::None => ::std::panic!(\"{{}} on {{}} is not a permission of this model\", info.value, info.resource.name),\n        }}\n    }}\n    /// The permission's description.\n    pub const fn info(self) -> &'static {P}::PermissionInfo {{\n        match self {{\n"
    ));
    for (_, names) in resources {
        out.push_str(&format!(
            "            Self::{}(permission) => permission.info(),\n",
            names.union_variant()
        ));
    }
    out.push_str(&format!(
        "        }}\n    }}\n    /// The permission's value.\n    pub const fn value(self) -> &'static str {{\n        self.info().value\n    }}\n    /// The resource it is on.\n    pub const fn resource(self) -> &'static {P}::ResourceInfo {{\n        self.info().resource\n    }}\n}}\n\nimpl ::std::convert::From<Permission> for &'static {P}::PermissionInfo {{\n    fn from(permission: Permission) -> Self {{\n        permission.info()\n    }}\n}}\n\nimpl ::std::fmt::Display for Permission {{\n    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        f.write_str(self.value())\n    }}\n}}\n"
    ));
    for (_, names) in resources {
        out.push_str(&format!(
            "\nimpl ::std::convert::From<{enum_}> for Permission {{\n    fn from(permission: {enum_}) -> Self {{\n        Self::{shape}(permission)\n    }}\n}}\n",
            enum_ = names.permission_enum(),
            shape = names.union_variant()
        ));
    }
    out
}

/// `Resource`: the typed reference a check names, by resource.
fn resource_union(resources: &[(&ShapeId, Names<'_>)]) -> String {
    let mut out = String::from(
        "\n/// The instance a check names, typed by resource.\n#[derive(Debug, Clone, PartialEq, Eq, Hash)]\npub enum Resource {\n",
    );
    for (_, names) in resources {
        out.push_str(&format!(
            "    {}({}),\n",
            names.union_variant(),
            names.reference()
        ));
    }
    out.push_str(&format!(
        "}}\n\n/// The check's resource, typed; an internal error for one this model does\n/// not declare.\nimpl ::std::convert::TryFrom<&{P}::ResourceRef> for Resource {{\n    type Error = {ERROR};\n    fn try_from(reference: &{P}::ResourceRef) -> ::std::result::Result<Self, Self::Error> {{\n        match reference.resource().name {{\n"
    ));
    for (_, names) in resources {
        out.push_str(&format!(
            "            {} => {}::try_from(reference).map(Self::{}),\n",
            literal(&names.resource.name),
            names.reference(),
            names.union_variant()
        ));
    }
    out.push_str(&format!(
        "            _ => ::std::result::Result::Err({ERROR}::internal(::std::format!(\"{{reference}} is not a resource of this model\"))),\n        }}\n    }}\n}}\n"
    ));
    out
}

/// The registry `SERVICE` carries: every primary, sorted by authorization
/// name and value.
pub(crate) fn registry(service: &Service) -> String {
    let mut primaries: Vec<(&str, &str, String)> = names(service)
        .iter()
        .flat_map(|(_, names)| {
            names.primaries().map(|p| {
                (
                    names.resource.name.as_str(),
                    p.value.as_str(),
                    format!("&permissions::{}", names.permission_static(p)),
                )
            })
        })
        .collect();
    if primaries.is_empty() {
        return String::new();
    }
    primaries.sort();
    format!(
        "\n.with_permissions(&[\n    {},\n])",
        primaries
            .into_iter()
            .map(|(_, _, path)| path)
            .collect::<Vec<_>>()
            .join(",\n    ")
    )
}

/// The service's own not-found error, when the model designates one: what
/// `Permissions` asks its `not_found_factory` for, and what is never cached.
pub(crate) fn not_found(service: &Service) -> String {
    service.not_found.as_ref().map_or_else(String::new, |id| {
        format!("\n.with_not_found({})", literal(&id.to_string()))
    })
}

/// `REQUIREMENTS`, `CHECKS` and `PUBLIC` for a server marker.
pub(crate) fn marker_items(
    service: &Service,
    op: &Operation,
    diagnostics: &mut Diagnostics,
) -> String {
    let mut out = String::new();
    // Only the model's explicit `public` exempts an operation, never an
    // empty scheme list (a service authenticated outside the model).
    if op.public {
        out.push_str("    const PUBLIC: bool = true;\n");
    }
    let requirements = match &op.requirements {
        // `Internal` exists only in a client projection; server bindings
        // read the full contract. Undeclared, the runtime refuses it.
        Requirements::Undeclared | Requirements::Internal => return out,
        // Likewise a client projection's: generating a server from it
        // would drop the clauses it removed.
        Requirements::Partial(_) => {
            diagnostics.push(
                &op.id,
                "the requirements were projected for a client audience; server bindings read the full contract",
            );
            return out;
        }
        Requirements::None => format!("{P}::Requirements::None"),
        Requirements::Pending { reason } => format!(
            "{P}::Requirements::Pending {{ reason: {} }}",
            literal(reason)
        ),
        Requirements::List(clauses) => {
            let items: Vec<String> = clauses
                .iter()
                .map(|clause| {
                    let permission = info_path(service, &clause.resource, &clause.permission)
                        .unwrap_or_else(|| {
                            diagnostics.push(
                                &op.id,
                                format!(
                                    "requires `{}` on `{}`, which declares no such permission",
                                    clause.permission, clause.resource
                                ),
                            );
                            "permissions::MISSING".to_owned()
                        });
                    format!(
                        "{P}::RequirementInfo {{ mode: {P}::Mode::{}, permission: &{permission}, conditional: {} }}",
                        mode(clause.mode),
                        clause.when.is_some()
                    )
                })
                .collect();
            format!(
                "{P}::Requirements::List(&[\n        {},\n    ])",
                items.join(",\n        ")
            )
        }
    };
    out.push_str(&format!(
        "    const REQUIREMENTS: {P}::Requirements = {requirements};\n"
    ));
    if let Some(checks) = checks(service, op, diagnostics) {
        out.push_str(&format!(
            "    #[allow(clippy::all, unreachable_patterns, unused_parens)]\n    const CHECKS: ::std::option::Option<{P}::Checks<Self::Input>> = ::std::option::Option::Some(|input, out| {{\n{checks}    }});\n"
        ));
    }
    out
}

fn mode(mode: Mode) -> &'static str {
    match mode {
        Mode::Check => "Check",
        Mode::Filter => "Filter",
        Mode::Handler => "Handler",
    }
}

/// `impl ExecuteQuery { pub const WRITE_ON_GRAPH: Obligation = .. }`: a
/// constant per `by handler` clause, for `permissions::discharge`, named
/// `<PERMISSION>_ON_<RESOURCE>`. Two clauses can spell the same name
/// (`lockOn` on `Room` and `lock` on `OnRoom`, or `read` on `Doc` twice):
/// the first keeps it and a later one gets the first free ordinal, `_2`,
/// `_3`, in declaration order, never a name an earlier clause holds (`read`
/// on `Doc_2` after a second `read` on `Doc`). Names depend only on earlier
/// clauses, so adding a clause never renames one a handler already uses.
pub(crate) fn obligations(service: &Service, op: &Operation) -> String {
    let mut body = String::new();
    let mut taken: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (index, clause) in op.requirements.clauses().iter().enumerate() {
        if clause.mode != Mode::Handler {
            continue;
        }
        let Some(path) = info_path(service, &clause.resource, &clause.permission) else {
            continue;
        };
        let resource = service.name_of(&clause.resource);
        let value = service
            .permissions
            .get(&clause.resource)
            .and_then(|r| r.permission(&clause.permission))
            .map_or(clause.permission.as_str(), |p| p.value.as_str());
        let mut text = format!("{} on {resource}, checked by the handler", code_span(value));
        if let Some(docs) = clause
            .docs
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
        {
            text.push_str(&format!(
                ":\n\n{}",
                crucible_codegen_common::docs::escape(docs)
            ));
        }
        let base = format!("{}_ON_{}", constant(&clause.permission), constant(resource));
        let name = if taken.contains(&base) {
            let (ordinal, name) = (2..)
                .map(|ordinal| (ordinal, format!("{base}_{ordinal}")))
                .find(|(_, name)| !taken.contains(name))
                .expect("an unbounded range");
            text.push_str(&format!(
                "\n\nNamed `_{ordinal}`: an earlier clause of this operation is `{base}`."
            ));
            name
        } else {
            base
        };
        taken.insert(name.clone());
        doc(&mut body, "    ", Some(&text));
        body.push_str(&format!(
            "    pub const {name}: {P}::Obligation = {P}::Obligation {{ operation: {}, index: {index}, permission: &{path} }};\n",
            literal(&op.id.to_string()),
        ));
    }
    if body.is_empty() {
        return String::new();
    }
    let allow = if op.notes.deprecated.is_some() {
        "#[allow(deprecated)]\n"
    } else {
        ""
    };
    format!(
        "/// The obligations its `by handler` clauses open: discharge each with\n/// `permissions::discharge`, or close it with `permissions::not_applicable`.\n/// Each is named `<PERMISSION>_ON_<RESOURCE>`; a later clause whose name\n/// an earlier one already spells gets `_2`, `_3`, in declaration order.\n{allow}impl {} {{\n{body}}}\n\n",
        op.name()
    )
}

/// The body of `CHECKS`: one push per `Check` or `Filter` clause, inside
/// its condition. `None` when the operation has no such clause.
fn checks(service: &Service, op: &Operation, diagnostics: &mut Diagnostics) -> Option<String> {
    let clauses = op.requirements.clauses();
    if !clauses.iter().any(|clause| clause.mode != Mode::Handler) {
        return None;
    }
    let mut out = String::new();
    let boxed = crate::types::boxed_members(service);
    let reader = Reader {
        service,
        boxed: &boxed,
        input: op.input.as_ref(),
    };
    for (index, clause) in clauses.iter().enumerate() {
        if clause.mode == Mode::Handler {
            continue;
        }
        let (Some(resource), Some(permission)) = (
            service.permissions.get(&clause.resource),
            info_path(service, &clause.resource, &clause.permission),
        ) else {
            continue;
        };
        let names = Names::new(service, &clause.resource, resource);
        let all = clause.bindings.last() == Some(&Binding::All);
        let bound = if all {
            &clause.bindings[..clause.bindings.len() - 1]
        } else {
            &clause.bindings[..]
        };
        // Each bound identifier: a value that is always there, or an
        // `Option` the condition makes present.
        let mut present = Vec::new();
        let mut optional = Vec::new();
        for (position, binding) in bound.iter().enumerate() {
            let Binding::Member(path) = binding else {
                diagnostics.push(&op.id, "`*` binds only a resource's own identifier");
                return None;
            };
            let Some(read) = reader.read(path) else {
                diagnostics.push(
                    &op.id,
                    format!("cannot bind `{}`: no such input member", path.join(".")),
                );
                return None;
            };
            if read.optional {
                let name = format!("b{position}");
                present.push(target(service, read.member, &name));
                optional.push((name, read.expression));
            } else {
                present.push(target(
                    service,
                    read.member,
                    &format!("&{}", read.expression),
                ));
            }
        }
        let constructor = if all { "all" } else { "new" };
        let ids = if present.is_empty() {
            format!("::std::iter::empty::<{P}::Target>()")
        } else {
            format!("[{}]", present.join(", "))
        };
        let complete = format!(
            "{P}::ResourceRef::{constructor}(&permissions::{}, {ids})",
            names.info
        );
        let push = match optional.as_slice() {
            [] => format!("out.push({P}::Check::declared({index}, &{permission}, {complete}));\n"),
            _ => {
                let (patterns, values): (Vec<String>, Vec<String>) = optional
                    .iter()
                    .map(|(name, expression)| {
                        (
                            format!("::std::option::Option::Some({name})"),
                            expression.clone(),
                        )
                    })
                    .unzip();
                let (pattern, value) = if optional.len() == 1 {
                    (patterns[0].clone(), values[0].clone())
                } else {
                    (
                        format!("({})", patterns.join(", ")),
                        format!("({})", values.join(", ")),
                    )
                };
                // A binding the condition should have made present binds
                // nothing: the runtime refuses that as a wiring error.
                format!(
                    "out.push({P}::Check::declared({index}, &{permission}, match {value} {{\n    {pattern} => {complete},\n    _ => {P}::ResourceRef::new(&permissions::{}, ::std::iter::empty::<{P}::Target>()),\n}}));\n",
                    names.info
                )
            }
        };
        match &clause.when {
            None => out.push_str(&indent_lines(&push, "        ")),
            Some(condition) => {
                let test = reader.condition(condition, diagnostics, &op.id)?;
                out.push_str(&format!("        if {test} {{\n"));
                out.push_str(&indent_lines(&push, "            "));
                out.push_str("        }\n");
            }
        }
    }
    Some(out)
}

fn indent_lines(text: &str, indent: &str) -> String {
    text.lines()
        .map(|line| format!("{indent}{line}\n"))
        .collect()
}

/// `Target::from(..)` of a bound identifier's value.
fn target(service: &Service, member: &Member, value: &str) -> String {
    let value = match service.shape(&member.target) {
        Some(Shape::Simple(simple)) if simple.kind == SimpleKind::String => value.to_owned(),
        Some(Shape::Enum(_)) => format!("{}.as_str()", value.trim_start_matches('&')),
        _ => format!("::std::string::ToString::to_string({value})"),
    };
    format!("{P}::Target::from({value})")
}

/// Reads input member paths as generated Rust.
struct Reader<'a> {
    service: &'a Service,
    boxed: &'a std::collections::BTreeSet<ShapeId>,
    input: Option<&'a ShapeId>,
}

struct Read<'a> {
    /// A place (`input.workspace_id`) when not optional, else an
    /// `Option<&T>` expression.
    expression: String,
    optional: bool,
    member: &'a Member,
}

impl<'a> Reader<'a> {
    fn read(&self, path: &[String]) -> Option<Read<'a>> {
        let mut current = self.input?.clone();
        let mut expression = "input".to_owned();
        let mut optional = false;
        // Whether `expression`, a place, holds a `Box`. An `Option`
        // expression always yields a plain reference.
        let mut place_boxed = false;
        let mut last = None;
        for segment in path {
            let shape = self.service.shape(&current)?;
            let (Shape::Structure(structure) | Shape::Union(structure)) = shape else {
                return None;
            };
            let member = structure.member(segment)?;
            let boxed = self.boxed.contains(&current.with_member(&member.name));
            if matches!(shape, Shape::Union(_)) {
                let union = format!("model::{}", self.service.name_of(&current));
                let arm = variant(&member.name);
                let value = if boxed { "&**v" } else { "v" };
                let matched = format!(
                    "{union}::{arm}(v) => ::std::option::Option::Some({value}), _ => ::std::option::Option::None"
                );
                expression = if optional {
                    format!("{expression}.and_then(|v| match v {{ {matched} }})")
                } else if place_boxed {
                    format!("match &*{expression} {{ {matched} }}")
                } else {
                    format!("match &{expression} {{ {matched} }}")
                };
                optional = true;
                place_boxed = false;
            } else {
                let name = field(&member.name);
                let unbox = if boxed { ".map(|v| &**v)" } else { "" };
                expression = match (optional, member.optional_on_server()) {
                    (false, false) => format!("{expression}.{name}"),
                    (false, true) => format!("{expression}.{name}.as_ref(){unbox}"),
                    (true, false) if boxed => format!("{expression}.map(|v| &*v.{name})"),
                    (true, false) => format!("{expression}.map(|v| &v.{name})"),
                    (true, true) => {
                        format!("{expression}.and_then(|v| v.{name}.as_ref()){unbox}")
                    }
                };
                place_boxed = !optional && !member.optional_on_server() && boxed;
                optional |= member.optional_on_server();
            }
            current = member.target.clone();
            last = Some(member);
        }
        Some(Read {
            expression,
            optional,
            member: last?,
        })
    }

    /// A `when` condition as a Rust `bool` expression.
    fn condition(
        &self,
        condition: &Condition,
        diagnostics: &mut Diagnostics,
        op: &ShapeId,
    ) -> Option<String> {
        let all = |parts: &[Condition], join: &str, diagnostics: &mut Diagnostics| {
            let parts: Option<Vec<String>> = parts
                .iter()
                .map(|part| self.condition(part, diagnostics, op))
                .collect();
            Some(format!("({})", parts?.join(join)))
        };
        match condition {
            Condition::And(parts) => all(parts, " && ", diagnostics),
            Condition::Or(parts) => all(parts, " || ", diagnostics),
            Condition::Not(inner) => Some(format!("!{}", self.condition(inner, diagnostics, op)?)),
            Condition::Has(path) => {
                let read = self.readable(path, diagnostics, op)?;
                Some(if read.optional {
                    format!("{}.is_some()", read.expression)
                } else {
                    "true".to_owned()
                })
            }
            Condition::Equals(path, value) => {
                self.compare(path, std::slice::from_ref(value), diagnostics, op)
            }
            Condition::In(path, values) => self.compare(path, values, diagnostics, op),
        }
    }

    fn readable(
        &self,
        path: &[String],
        diagnostics: &mut Diagnostics,
        op: &ShapeId,
    ) -> Option<Read<'a>> {
        let read = self.read(path);
        if read.is_none() {
            diagnostics.push(
                op,
                format!("`when` reads `{}`, which the input lacks", path.join(".")),
            );
        }
        read
    }

    /// `path == value`, or `path` equal to any of `values`: false when the
    /// path is absent.
    fn compare(
        &self,
        path: &[String],
        values: &[Value],
        diagnostics: &mut Diagnostics,
        op: &ShapeId,
    ) -> Option<String> {
        let read = self.readable(path, diagnostics, op)?;
        let shape = self.service.shape(&read.member.target);
        let mut tests = Vec::new();
        for value in values {
            let test = match (shape, value) {
                (Some(Shape::Enum(_)), Value::String(text)) => {
                    format!("v.as_str() == {}", literal(text))
                }
                (Some(Shape::IntEnum(_)), Value::Number(n)) => format!("v.as_i32() == {n}"),
                (Some(Shape::Simple(simple)), value) => match (simple.kind, value) {
                    (SimpleKind::String, Value::String(text)) => {
                        format!("v.as_str() == {}", literal(text))
                    }
                    (SimpleKind::Boolean, Value::Bool(true)) => "*v".to_owned(),
                    (SimpleKind::Boolean, Value::Bool(false)) => "!*v".to_owned(),
                    (
                        SimpleKind::Byte
                        | SimpleKind::Short
                        | SimpleKind::Integer
                        | SimpleKind::Long,
                        Value::Number(n),
                    ) if n.is_i64() => format!("i64::from(*v) == {n}"),
                    // Compared in the member's own type: the literal is
                    // the nearest value of it, as the decoded input is.
                    (SimpleKind::Float, Value::Number(n)) => {
                        let n = n.as_f64().unwrap_or_default();
                        // Beyond `f32`'s range the literal has no nearest
                        // value: `1e300_f32` does not compile.
                        #[allow(clippy::cast_possible_truncation)]
                        if !(n as f32).is_finite() {
                            diagnostics.push(
                                op,
                                format!(
                                    "`when` compares the float `{}` with {n:?}, outside the range of a float",
                                    path.join(".")
                                ),
                            );
                            return None;
                        }
                        format!("*v == {n:?}_f32")
                    }
                    (SimpleKind::Double, Value::Number(n)) => {
                        let n = n.as_f64().unwrap_or_default();
                        format!("*v == {n:?}_f64")
                    }
                    _ => {
                        diagnostics.push(
                            op,
                            format!(
                                "`when` compares `{}` with {value}, which the Rust server cannot evaluate",
                                path.join(".")
                            ),
                        );
                        return None;
                    }
                },
                _ => {
                    diagnostics.push(
                        op,
                        format!(
                            "`when` compares `{}` with {value}, which the Rust server cannot evaluate",
                            path.join(".")
                        ),
                    );
                    return None;
                }
            };
            tests.push(test);
        }
        let test = tests.join(" || ");
        Some(if read.optional {
            format!("{}.is_some_and(|v| {test})", read.expression)
        } else {
            format!("{{ let v = &{}; {test} }}", read.expression)
        })
    }
}
