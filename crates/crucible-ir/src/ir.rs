//! One service, lowered: what every emitter reads.
//!
//! The model's rules are settled here, once, so no emitter re-derives them:
//! which members are optional, the effective constraints on a member, where
//! each member travels in a request or response, the errors an operation can
//! return, and its pagination. Traits the IR does not model are kept raw in
//! each `traits` map, so a project's own traits reach the emitters intact.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub use crate::doc_notes::{Deprecation, DocNotes};
pub use crate::extensions::*;
pub use crate::recursion::{RecursiveGroup, groups as recursive_groups};
pub use crate::{Protocol, ProtocolSettings};
pub use serde_json::{Number, Value};

use crate::shape_id::ShapeId;

pub mod permissions;
pub use permissions::{
    Condition, Mode, Permission, PermissionTypes, Requirement, Requirements, ResourcePermissions,
    Tier,
};

pub type Traits = BTreeMap<ShapeId, Value>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Service {
    /// Visibility settled from `internal` during lowering.
    pub internal: bool,
    pub recursive_groups: Vec<RecursiveGroup>,
    pub protocol: Protocol,
    /// Errors declared on the service, independently of operation inheritance.
    pub errors: Vec<ShapeId>,
    pub protocol_settings: ProtocolSettings,
    pub requirements: Vec<crate::FeatureUse>,
    pub id: ShapeId,
    pub version: String,
    pub title: Option<String>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    /// Auth schemes the service declares, in declaration order.
    pub auth: Vec<ShapeId>,
    pub auth_schemes: BTreeMap<ShapeId, AuthScheme>,
    /// Sorted by name, so output does not depend on model file order.
    pub operations: Vec<Operation>,
    /// Resources bound to this service, including nested resources, with their
    /// design-time references and bindings.
    pub resources: BTreeMap<ShapeId, Resource>,
    /// Direct service bindings, sorted by shape ID.
    pub resource_roots: Vec<ShapeId>,
    /// Design-time resource references, keyed by the referencing shape.
    pub resource_references: BTreeMap<ShapeId, Vec<ResourceReference>>,
    /// Every shape an operation reaches, prelude shapes included. A type
    /// only a resource's identifier or property names is not among them.
    pub shapes: BTreeMap<ShapeId, Shape>,
    pub traits: Traits,
    /// The older API versions this service still serves, oldest first, each
    /// with the steps that take it to the next (docs/api-versions.md). Empty
    /// for a service with no migrations; `version` is the current one.
    pub versions: Vec<ApiVersion>,
    /// The release a generated client names in its `Crucible-Version`
    /// header (docs/api-versions-clients.md): the service's `version`, for
    /// a `.cru` service, whose versions are releases. `None` when the
    /// `version` cannot travel in a header, or for IR built by hand.
    pub api_version: Option<String>,
    /// Custom traits whose definition is `@internal`. An external view drops
    /// their applications; nothing else reads it.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub internal_traits: BTreeSet<ShapeId>,
    /// The permissions each resource declares, keyed by resource; only
    /// resources that declare any (docs/permissions.md §6.1). Kept apart
    /// from `resources`, which older release snapshots leave empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub permissions: BTreeMap<ShapeId, ResourcePermissions>,
    /// The `@permissionTypes` enum, if the service has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_types: Option<PermissionTypes>,
    /// The error the service answers a hidden or missing resource with,
    /// when it designates its own (`.cru` `notFound ResourceNotFoundException`
    /// in the service block): what check 17 attaches to every operation with
    /// something to hide, instead of `crucible.errors#NotFoundException`.
    /// `None` is the framework's. Read it through [`Service::not_found_error`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_found: Option<ShapeId>,
    /// The policy rule violations the package excepts, each with its reason
    /// (docs/idl/decisions.md, decision 47): what a reviewer of the compiled
    /// model sees was accepted. Empty for a model with none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exceptions: Vec<PolicyException>,
    /// What each `[Name]` doc link in this service's documentation names
    /// (decision 80 in docs/idl/decisions.md), keyed by its label as written
    /// (`CreateApiKey`, `City.name`): an operation, shape, member, enum
    /// value, resource or the service, by id. `docs` keep the Markdown as
    /// written; [`crate::doc_links::links`] finds the labels in it. A label
    /// with no entry, such as a prelude type's or one whose target an
    /// audience projection removed, is plain text.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub doc_links: BTreeMap<String, ShapeId>,
    /// For each `[Operation.member]` label in [`Service::doc_links`], the
    /// operation it names: its target is a member of the operation's input
    /// or output structure, which another operation may share.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub doc_link_operations: BTreeMap<String, ShapeId>,
}

/// One `except Rule for Target because ".."` the compiler accepted: the rule
/// really fired on the target, and the package was allowed to except it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PolicyException {
    /// The rule's name.
    pub rule: String,
    /// The namespace of the package that declares the rule.
    pub rule_package: String,
    /// The name of the shape, operation or resource excepted.
    pub target: String,
    pub reason: String,
    /// The namespace of the package that declares the exception.
    pub package: String,
}

/// An older API version: its release's model, and how a request or a
/// response crosses to the next version.
///
/// Never part of a release snapshot: `snapshot::write` clears
/// `Service::versions`, so the steps are no on-disk format. A field with a
/// natural empty value still takes `#[serde(default)]`; a shape id or a
/// route has none.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiVersion {
    /// The release's `version`, such as `2026-09-04`.
    pub name: String,
    /// That release's lowered model: its routes, shapes and errors.
    pub service: Box<Service>,
    /// The migration from this version to the next, in declaration order.
    pub steps: Vec<VersionStep>,
    /// After this date the version may be refused (`YYYY-MM-DD`).
    pub sunset: Option<String>,
    /// The `///` documentation of the `sunset` step: why and how the
    /// version ends, for the reference and upgrade guides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sunset_docs: Option<String>,
    /// The `///` documentation of the migration from this version to the
    /// next: what changed, for the reference and upgrade guides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    /// The operations this version has that the next dropped without
    /// `remove operation`, under a `compatibility.operation` exception: not
    /// served at this version or before (decision 70). The compiler refuses
    /// such a drop without the exception.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<ShapeId>,
}

/// One checked migration step, its paths resolved to shape ids. `old`
/// names a shape in this version's model, `new` in the next version's;
/// each step carries both sides, so neither needs looking up by the
/// other's id. `docs` is the step's `///` documentation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum VersionStep {
    /// A member renamed: `old.from` became `new.to`.
    Rename {
        old: ShapeId,
        new: ShapeId,
        from: String,
        to: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        docs: Option<String>,
    },
    /// An optional input member became required; an older request that
    /// omits it gets `default`.
    Require {
        /// The structure as this version declares it.
        old: ShapeId,
        new: ShapeId,
        member: String,
        /// The member's name in this version, when the same migration
        /// renamed it; empty when it kept `member`.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        from: String,
        default: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        docs: Option<String>,
    },
    /// An enum value added; older responses see `downgrade` instead.
    AddEnumValue {
        /// The enum as this version declares it, which has `downgrade`.
        old: ShapeId,
        new: ShapeId,
        value: String,
        downgrade: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        docs: Option<String>,
    },
    /// An error added to an operation; older clients get `downgrade`, its
    /// members filled from the new error's (`downgrade member <- new member`).
    AddError {
        operation: ShapeId,
        error: ShapeId,
        downgrade: ShapeId,
        members: Vec<(String, String)>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        docs: Option<String>,
    },
    /// An operation moved; this version reaches it at `old_method`
    /// `old_uri`, the route `move Op from ..` names.
    Move {
        operation: ShapeId,
        old_method: String,
        old_uri: Uri,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        docs: Option<String>,
    },
    /// A behaviour change the handler branches on (`behavior Op.name`).
    Behavior {
        operation: ShapeId,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        docs: Option<String>,
    },
    /// A step a hook the service writes, or a declared `conversion`, serves
    /// (`reshape`, `retype`, `require .. by hook`, `remove operation`).
    Hooked {
        /// The step's verb, for messages.
        kind: String,
        /// The operations it affects.
        operations: Vec<ShapeId>,
        hook: Hook,
        /// `by async hook`: the hook may do I/O.
        is_async: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        docs: Option<String>,
    },
}

impl VersionStep {
    /// The step's `///` documentation, if it has any.
    pub fn docs(&self) -> Option<&str> {
        match self {
            Self::Rename { docs, .. }
            | Self::Require { docs, .. }
            | Self::AddEnumValue { docs, .. }
            | Self::AddError { docs, .. }
            | Self::Move { docs, .. }
            | Self::Behavior { docs, .. }
            | Self::Hooked { docs, .. } => docs.as_deref(),
        }
    }
}

/// What a hook converts. `old` names a shape in the older version's model,
/// `new` in the next version's.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Hook {
    /// `reshape S { a, b } -> m`: the whole structure, each way it travels.
    Reshape {
        old: ShapeId,
        new: ShapeId,
        /// The members of `old` the step replaced, as written.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        members: Vec<String>,
        /// The member of `new` they became.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        into: String,
    },
    /// `retype S.m -> T`: the member's value, each way it travels, by the
    /// service's hook or by a declared `conversion`.
    Retype {
        old: ShapeId,
        new: ShapeId,
        member: String,
        /// The member's name in the older version, when the same migration
        /// renamed it; empty when it kept `member`.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        from: String,
        /// What converts the value.
        #[serde(default, skip_serializing_if = "RetypeBy::is_hook")]
        by: RetypeBy,
    },
    /// `require S.m by hook`: the value an older request could not send,
    /// computed from the rest of it.
    Require {
        old: ShapeId,
        new: ShapeId,
        member: String,
        /// The member's name in the older version, when the same migration
        /// renamed it; empty when it kept `member`. Always empty while the
        /// compiler refuses a hooked requirement on a renamed member.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        from: String,
    },
    /// `remove operation Op`: the older versions' own handler.
    RemoveOperation { operation: ShapeId },
}

/// What converts a retyped member's value between two versions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RetypeBy {
    /// `retype S.m -> T by hook`: a method the service writes.
    #[default]
    Hook,
    /// `retype S.m -> T` without `by hook`: a declared `conversion`.
    Conversion,
}

impl RetypeBy {
    fn is_hook(&self) -> bool {
        *self == Self::Hook
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Resource {
    /// Whether the resource is left out of the external contract. A `.cru`
    /// resource cannot be internal, so the language always writes `false`.
    pub internal: bool,
    pub id: ShapeId,
    pub identifiers: BTreeMap<String, ShapeId>,
    pub properties: BTreeMap<String, ShapeId>,
    pub resources: Vec<ShapeId>,
    pub lifecycle: ResourceLifecycle,
    pub operations: Vec<ShapeId>,
    pub collection_operations: Vec<ShapeId>,
    /// Bindings are per resource: an operation may belong to several resources.
    pub bindings: BTreeMap<ShapeId, ResourceOperationBindings>,
    pub traits: Traits,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ResourceLifecycle {
    pub create: Option<ShapeId>,
    pub put: Option<ShapeId>,
    pub read: Option<ShapeId>,
    pub update: Option<ShapeId>,
    pub delete: Option<ShapeId>,
    pub list: Option<ShapeId>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ResourceOperationBindings {
    pub input: ResourceMemberBindings,
    pub output: ResourceMemberBindings,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ResourceMemberBindings {
    /// Resource identifier name to member path, starting at the input/output.
    pub identifiers: BTreeMap<String, Vec<String>>,
    /// Resource property name to member path, starting at the input/output.
    pub properties: BTreeMap<String, Vec<String>>,
    /// Collection member name to bindings relative to each element: each
    /// item of a list, or each value of a map. These are repeated resource
    /// instances, not scalar member paths.
    pub collections: BTreeMap<String, ResourceMemberBindings>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResourceReference {
    pub resource: ShapeId,
    pub service: Option<ShapeId>,
    /// Identifier names to member paths; an empty path denotes the string itself.
    pub identifiers: BTreeMap<String, Vec<String>>,
    pub rel: Option<String>,
    /// An external reference names a resource of another service and may
    /// not resolve in this model.
    pub resolved: bool,
}

/// Whether `id` is a standard trait rather than a project's own: one in the
/// prelude namespace or in `crucible.protocols`. The lowering uses them
/// while it works; the IR carries their facts as typed fields.
pub fn is_standard_trait(id: &ShapeId) -> bool {
    id.is_prelude() || id.namespace() == "crucible.protocols"
}

impl Service {
    /// The error a hidden resource is refused with (docs/permissions.md
    /// check 17): the service's designated [`not_found`](Self::not_found),
    /// or `crucible.errors#NotFoundException`.
    pub fn not_found_error(&self) -> ShapeId {
        self.not_found
            .clone()
            .unwrap_or_else(crate::validate::not_found_id)
    }
    /// Every permission the service declares, sorted by authorization name,
    /// then value: the registry docs, SDKs and discovery publish. Aliases
    /// are included; [`Permission::alias_of`] tells them apart.
    pub fn all_permissions(&self) -> Vec<(&ResourcePermissions, &Permission)> {
        let mut all: Vec<_> = self
            .permissions
            .values()
            .flat_map(|resource| resource.permissions.iter().map(move |p| (resource, p)))
            .collect();
        all.sort_by(|(ra, a), (rb, b)| (&ra.name, &a.value).cmp(&(&rb.name, &b.value)));
        all
    }
    /// Emitters must check their own capabilities even for externally lowered IR.
    pub fn require_capabilities(
        &self,
        capabilities: &crate::Capabilities,
    ) -> Result<(), crate::Diagnostics> {
        capabilities.check(&self.requirements)
    }
    /// Keeps only project metadata in every raw `traits` map, dropping
    /// the standard vocabulary the lowering used while it worked (the
    /// prelude's and `crucible.protocols`), which the IR carries as typed
    /// fields (decision 43). The `.cru` front end emits IR this way.
    pub fn project_metadata_only(&mut self) {
        self.retain_traits(|id| !is_standard_trait(id));
    }

    /// Keeps the entries of every raw `traits` map that `keep` accepts.
    pub fn retain_traits(&mut self, keep: impl Fn(&ShapeId) -> bool) {
        let strip = |traits: &mut Traits| traits.retain(|id, _| keep(id));
        strip(&mut self.traits);
        for operation in &mut self.operations {
            strip(&mut operation.traits);
        }
        for resource in self.resources.values_mut() {
            strip(&mut resource.traits);
        }
        for shape in self.shapes.values_mut() {
            match shape {
                Shape::Unit { .. } => {}
                Shape::Simple(s) => strip(&mut s.traits),
                Shape::Enum(s) => {
                    strip(&mut s.traits);
                    for value in &mut s.values {
                        strip(&mut value.traits);
                    }
                }
                Shape::IntEnum(s) => {
                    strip(&mut s.traits);
                    for value in &mut s.values {
                        strip(&mut value.traits);
                    }
                }
                Shape::List(s) => {
                    strip(&mut s.traits);
                    strip(&mut s.member.traits);
                }
                Shape::Map(s) => {
                    strip(&mut s.traits);
                    strip(&mut s.key.traits);
                    strip(&mut s.value.traits);
                }
                Shape::Structure(s) | Shape::Union(s) => {
                    strip(&mut s.traits);
                    for member in &mut s.members {
                        strip(&mut member.traits);
                    }
                }
            }
        }
    }

    /// Whether `scheme`, one of the service's auth scheme ids, is bearer auth.
    pub fn is_bearer(&self, scheme: &ShapeId) -> bool {
        matches!(self.auth_schemes.get(scheme), Some(AuthScheme::Bearer))
    }

    /// Effective sensitivity: a member or its target can forbid logging.
    pub fn sensitive(&self, member: &Member) -> bool {
        member.sensitive || self.shape(&member.target).is_some_and(Shape::sensitive)
    }

    pub fn shape(&self, id: &ShapeId) -> Option<&Shape> {
        self.shapes.get(id)
    }

    /// The service with the shape `id` called `name` (in its namespace)
    /// wherever the IR names it: test support for generators' name
    /// diagnostics, which reach names a `.cru` model cannot spell. The
    /// wire codes the IR settled are unchanged.
    #[doc(hidden)]
    pub fn rename_shape(&mut self, id: &ShapeId, name: &str) {
        fn walk(value: &mut Value, old: &str, new: &str) {
            let renamed = |text: &str| {
                if text == old {
                    Some(new.to_owned())
                } else {
                    text.strip_prefix(old)
                        .filter(|rest| rest.starts_with('$'))
                        .map(|rest| format!("{new}{rest}"))
                }
            };
            match value {
                Value::String(text) => {
                    if let Some(text_new) = renamed(text) {
                        *text = text_new;
                    }
                }
                Value::Array(items) => items.iter_mut().for_each(|item| walk(item, old, new)),
                Value::Object(fields) => {
                    for (key, mut field) in std::mem::take(fields) {
                        walk(&mut field, old, new);
                        fields.insert(renamed(&key).unwrap_or(key), field);
                    }
                }
                _ => {}
            }
        }
        let new = ShapeId::new(id.namespace(), name);
        assert!(
            !self.shapes.contains_key(&new),
            "{new} exists; renaming {id} to it would merge two shapes"
        );
        let mut value = serde_json::to_value(&*self).expect("the IR serializes");
        walk(&mut value, &id.to_string(), &new.to_string());
        *self = serde_json::from_value(value).expect("the renamed IR reads back");
    }

    /// The name a shape goes by in this service: the name in its id.
    pub fn name_of<'a>(&'a self, id: &'a ShapeId) -> &'a str {
        id.name()
    }

    /// Timestamp encoding inside a document; HTTP annotations have no effect here.
    pub fn document_timestamp_format(&self, member: &Member) -> TimestampFormat {
        member
            .timestamp_format
            .or_else(|| match self.shape(&member.target) {
                Some(Shape::Simple(simple)) => simple.timestamp_format,
                _ => None,
            })
            .unwrap_or_else(|| self.protocol.timestamp_format(false))
    }

    pub fn document_wire_default(&self, member: &Member) -> Option<Value> {
        let value = member.default.as_ref()?;
        if matches!(
            self.shape(&member.target),
            Some(Shape::Simple(Simple {
                kind: SimpleKind::Timestamp,
                ..
            }))
        ) {
            crate::timestamp_default::wire_value(value, self.document_timestamp_format(member))
        } else {
            Some(self.wire_example(member, value, None))
        }
    }

    /// Timestamp format resolved during lowering, including only effective HTTP bindings.
    pub fn timestamp_format(&self, member: &Member) -> TimestampFormat {
        member.wire_timestamp_format
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(
    clippy::large_enum_variant,
    reason = "built once per shape and only read; boxing would only add indirection for emitters"
)]
pub enum Shape {
    /// `crucible.api#Unit`: no value, as an operation's input or output or as
    /// a union member that carries nothing.
    Unit {
        internal: bool,
    },
    Simple(Simple),
    Enum(Enum),
    IntEnum(IntEnum),
    List(List),
    Map(Map),
    Structure(Structure),
    Union(Structure),
}

impl Shape {
    pub fn internal(&self) -> bool {
        match self {
            Self::Unit { internal } => *internal,
            Self::Simple(s) => s.internal,
            Self::Enum(s) => s.internal,
            Self::IntEnum(s) => s.internal,
            Self::List(s) => s.internal,
            Self::Map(s) => s.internal,
            Self::Structure(s) | Self::Union(s) => s.internal,
        }
    }

    /// Documentation-only traits; `Unit` carries none.
    pub fn notes(&self) -> Option<&DocNotes> {
        match self {
            Self::Unit { .. } => None,
            Self::Simple(s) => Some(&s.notes),
            Self::Enum(s) => Some(&s.notes),
            Self::IntEnum(s) => Some(&s.notes),
            Self::List(s) => Some(&s.notes),
            Self::Map(s) => Some(&s.notes),
            Self::Structure(s) | Self::Union(s) => Some(&s.notes),
        }
    }

    pub fn members(&self) -> Vec<&Member> {
        match self {
            Self::Structure(s) | Self::Union(s) => s.members.iter().collect(),
            Self::List(s) => vec![&s.member],
            Self::Map(s) => vec![&s.key, &s.value],
            _ => vec![],
        }
    }

    pub fn sensitive(&self) -> bool {
        match self {
            Self::Unit { .. } => false,
            Self::Simple(s) => s.sensitive,
            Self::Enum(s) => s.sensitive,
            Self::IntEnum(s) => s.sensitive,
            Self::List(s) => s.sensitive,
            Self::Map(s) => s.sensitive,
            Self::Structure(s) | Self::Union(s) => s.sensitive,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Simple {
    /// Visibility settled from `internal` during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    pub kind: SimpleKind,
    pub constraints: Constraints,
    pub timestamp_format: Option<TimestampFormat>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SimpleKind {
    Boolean,
    String,
    Byte,
    Short,
    Integer,
    Long,
    Float,
    Double,
    BigInteger,
    BigDecimal,
    Timestamp,
    Document,
    /// Bytes: base64 in a JSON body, raw as an `@httpPayload`.
    Blob,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Enum {
    /// Visibility settled from `internal` during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    pub values: Vec<EnumValue>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnumValue {
    /// Visibility settled from `internal` during lowering.
    pub internal: bool,
    /// The member name, for a language symbol.
    pub name: String,
    /// What goes on the wire.
    pub value: String,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    /// The value's own traits, such as project metadata on one enum value.
    pub traits: Traits,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IntEnum {
    /// Visibility settled from `internal` during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    pub values: Vec<IntEnumValue>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IntEnumValue {
    /// Visibility settled from `internal` during lowering.
    pub internal: bool,
    pub name: String,
    pub value: i32,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    /// The value's own traits, such as project metadata on one enum value.
    pub traits: Traits,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct List {
    /// Visibility settled from `internal` during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    /// Elements may be null.
    pub sparse: bool,
    pub member: Member,
    pub constraints: Constraints,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Map {
    /// Visibility settled from `internal` during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    /// Values may be null.
    pub sparse: bool,
    pub key: Member,
    pub value: Member,
    pub constraints: Constraints,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Structure {
    /// Visibility settled from `internal` during lowering.
    pub internal: bool,
    /// `@sensitive`: values must not be logged; see [`Service::sensitive`].
    pub sensitive: bool,
    /// Refuses members it does not define (`crucible.protocols#strict`, or a
    /// `strict` structure or input). Server side only.
    pub strict: bool,
    /// An operation's own input structure: its anonymous `input` body. Generators use it to type members as the model says.
    pub input: bool,
    pub members: Vec<Member>,
    /// Present on a shape marked `@error`.
    pub error: Option<ErrorInfo>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

impl Structure {
    pub fn sensitive(&self) -> bool {
        self.sensitive
    }

    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|member| member.name == name)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Member {
    /// Visibility settled from `internal` during lowering.
    pub internal: bool,
    /// `@sensitive` on the member itself; see [`Service::sensitive`].
    pub sensitive: bool,
    pub name: String,
    pub target: ShapeId,
    pub required: bool,
    pub default: Option<Value>,
    /// The default represented in the member's resolved wire format.
    pub wire_default: Option<Value>,
    /// The input member a client fills with a fresh idempotency token.
    pub idempotency_token: bool,
    /// The infrastructure mutability of the property this member carries.
    /// A `.cru` package sets it from the property's keyword on every member
    /// that carries the value (views, outputs, and the inputs of `creates`,
    /// `puts` and `updates`), and from `@mutability` on a member that binds
    /// no property. `None` declares nothing: a filter binding, an
    /// identifier, or a member that binds no property and has no
    /// `@mutability`.
    pub mutability: Option<Mutability>,
    /// The target's constraints with the member's own laid over them.
    pub constraints: Constraints,
    /// The member's own `@timestampFormat`; see [`Service::timestamp_format`].
    pub timestamp_format: Option<TimestampFormat>,
    /// Effective timestamp format for this member's top-level wire location.
    /// Nested document encoding uses the document format instead.
    pub wire_timestamp_format: TimestampFormat,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub traits: Traits,
}

impl Member {
    /// Whether a value must carry the member: it is required and the server
    /// has no default to fill in. An example or a request without it fails.
    pub fn required_without_default(&self) -> bool {
        self.required && self.default.is_none()
    }

    /// Whether a value may leave the member out: it is neither required
    /// nor defaulted.
    pub fn optional_on_server(&self) -> bool {
        !self.required && self.default.is_none()
    }

    /// Whether a client may leave the member out: the same as on the
    /// server, since a `.cru` member's optionality is one fact for both
    /// (decision 20 in docs/idl/decisions.md).
    pub fn optional_on_client(&self) -> bool {
        self.optional_on_server()
    }
}

/// How provisioning may change a resource property, in CloudFormation's
/// vocabulary, which the CDK and Terraform generators map onto their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mutability {
    /// Set at create, changed by update, read back.
    Full,
    /// Set at create only, never read back.
    Create,
    /// Set at create only, read back: changing it replaces the resource.
    CreateAndRead,
    /// Read only: the service assigns it.
    Read,
    /// Written at create and update, never read back (a secret, say).
    Write,
}

impl Mutability {
    /// The `aws.cloudformation#cfnMutability` spelling.
    pub fn from_cfn(value: &str) -> Option<Self> {
        Some(match value {
            "full" => Self::Full,
            "create" => Self::Create,
            "create-and-read" => Self::CreateAndRead,
            "read" => Self::Read,
            "write" => Self::Write,
            _ => return None,
        })
    }

    /// Whether changing the property after creation replaces the resource.
    pub fn create_only(self) -> bool {
        matches!(self, Self::Create | Self::CreateAndRead)
    }

    /// Whether reads never return the property.
    pub fn write_only(self) -> bool {
        matches!(self, Self::Create | Self::Write)
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Constraints {
    pub length: Option<Bounds<u64>>,
    pub range: Option<Bounds<Number>>,
    pub pattern: Option<String>,
    pub unique_items: bool,
}

impl Constraints {
    /// Translate the ECMA-262 pattern for Rust's Unicode scalar regex engine.
    /// The original pattern remains available for other targets and diagnostics.
    pub fn rust_pattern(&self) -> Result<Option<String>, String> {
        self.pattern
            .as_deref()
            .map(crate::pattern::rust)
            .transpose()
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Bounds<T> {
    pub min: Option<T>,
    pub max: Option<T>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ErrorInfo {
    pub protocol_headers: BTreeMap<String, String>,
    /// Selected protocol's body discriminator; REST JSON uses its header instead.
    pub body_discriminator: Option<String>,
    pub discriminator_header: Option<String>,
    pub timestamp_formats: BTreeMap<String, TimestampFormat>,
    pub wire_code: String,
    pub payload: Option<Payload>,
    pub header_encodings: BTreeMap<String, HeaderEncoding>,
    pub fault: Fault,
    pub status: u16,
    pub retryable: Option<Retryable>,
    /// Members sent as response headers; the rest form the body.
    pub headers: Vec<Binding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Fault {
    Client,
    Server,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Retryable {
    pub throttling: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Operation {
    /// Fixed request headers required by the selected protocol.
    pub protocol_headers: BTreeMap<String, String>,
    pub response_protocol_headers: BTreeMap<String, String>,
    /// Visibility settled from `internal` during lowering.
    pub internal: bool,
    pub id: ShapeId,
    /// Effective authentication schemes, after service and operation overrides.
    pub auth: Vec<ShapeId>,
    /// Declared public: the `.cru` `public` modifier. Empty `auth` alone does not say so: a service that
    /// declares no scheme leaves every operation without one
    /// (docs/permissions.md §6.1).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub public: bool,
    /// The resource that binds the operation; `None` when the service does.
    pub resource: Option<ShapeId>,
    pub docs: Option<String>,
    /// `@deprecated`, `@since`, `@unstable` and `@externalDocumentation`.
    pub notes: DocNotes,
    pub method: String,
    pub uri: Uri,
    /// The status of a successful response.
    pub status: u16,
    pub input: Option<ShapeId>,
    pub output: Option<ShapeId>,
    /// The operation's own errors, then the service's.
    pub errors: Vec<ShapeId>,
    pub readonly: bool,
    pub idempotent: bool,
    /// The input member a client fills with a fresh token when it is absent.
    pub idempotency_token: Option<String>,
    pub paginated: Option<Paginated>,
    /// `@examples`, validated against the operation's shapes, in model order.
    pub examples: Vec<Example>,
    pub request: Bindings,
    pub response: Bindings,
    pub traits: Traits,
    /// What the operation requires, beside `readonly`, `idempotent` and
    /// `errors` (docs/permissions.md §6.1).
    #[serde(default, skip_serializing_if = "Requirements::is_undeclared")]
    pub requirements: Requirements,
}

/// One `@examples` entry. `input`, `output` and `error.content` are model
/// values: members are named as in the model and timestamps are as written.
/// [`Service::wire_example`] renders a member's value as a document carries it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Example {
    pub title: String,
    pub docs: Option<String>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error: Option<ExampleError>,
    /// Constraint violations (length, range, pattern, uniqueness, required
    /// members) were permitted in this example.
    pub allow_constraint_errors: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExampleError {
    pub shape: ShapeId,
    pub content: Value,
}

impl Operation {
    pub fn name(&self) -> &str {
        self.id.name()
    }
}

/// Where each member of an input or output structure travels.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Bindings {
    pub timestamp_formats: BTreeMap<String, TimestampFormat>,
    pub header_encodings: BTreeMap<String, HeaderEncoding>,
    pub labels: Vec<String>,
    pub query: Vec<Binding>,
    pub headers: Vec<Binding>,
    /// Members serialized into the JSON body, in member order. Empty when
    /// there is a payload.
    pub body: Vec<String>,
    /// The member that is the whole body, raw (`@httpPayload` on a blob).
    pub payload: Option<Payload>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Payload {
    pub kind: PayloadKind,
    pub member: String,
    /// The target's `@mediaType`, else `application/octet-stream`: the
    /// `Content-Type` a sender gives the body.
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Binding {
    pub member: String,
    /// The query parameter or header name.
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimestampFormat {
    DateTime,
    HttpDate,
    EpochSeconds,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Paginated {
    pub input_token: String,
    pub output_token: String,
    pub items: Option<String>,
    pub page_size: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Uri {
    pub segments: Vec<Segment>,
    /// Literal query parameters in the pattern, such as `?x-id=Get`.
    pub query: Vec<(String, Option<String>)>,
    /// Labels whose values preserve slash separators when percent-encoded.
    pub greedy_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Segment {
    Literal(String),
    Label(String),
}

impl Uri {
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().filter_map(|segment| match segment {
            Segment::Label(name) => Some(name.as_str()),
            Segment::Literal(_) => None,
        })
    }
}

impl fmt::Display for Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            f.write_str("/")?;
        }
        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => write!(f, "/{text}")?,
                Segment::Label(name) => write!(
                    f,
                    "/{{{name}{}}}",
                    if self.greedy_labels.contains(name) {
                        "+"
                    } else {
                        ""
                    }
                )?,
            }
        }
        for (index, (key, value)) in self.query.iter().enumerate() {
            f.write_str(if index == 0 { "?" } else { "&" })?;
            match value {
                Some(value) => write!(f, "{key}={value}")?,
                None => f.write_str(key)?,
            }
        }
        Ok(())
    }
}
