//! Lowering a parsed package to `ir::Service`.
//!
//! Lowering settles the semantics the generators share (bindings, effective
//! constraints, timestamp formats, error envelopes). It works with
//! Smithy-style trait ids internally, the IR's trait vocabulary; the finished
//! IR keeps only project metadata in its raw `traits` maps, because every
//! generator reads typed fields (decision 63).
//!
//! Constructs the compiler cannot lower yet are diagnostics, never skipped.

use std::collections::{BTreeMap, BTreeSet};

use crucible_smithy::ir::{
    AuthScheme, Binding, Bindings, Bounds, Constraints, Deprecation, DocNotes, Enum,
    EnumValue as IrEnumValue, ErrorInfo, Example as IrExample, ExampleError, Fault, HeaderEncoding,
    IntEnum, IntEnumValue, List, Map, Member as IrMember, Mutability, Operation as IrOperation,
    Paginated, Payload, PayloadKind, Retryable, Segment, Service, Shape, Simple, SimpleKind,
    Structure as IrStructure, TimestampFormat, Traits, Uri,
};
use crucible_smithy::{Feature, FeatureUse, Protocol, ProtocolSettings, ShapeId};
use serde_json::{Map as JsonMap, Number, Value as Json};

use crate::Parsed;
use crate::ast::{
    Annotation, Attachment, Body, BodyKind, ClauseValue, Constraint, ConstraintValue, Decl, Doc,
    EnumValue, ErrorEntry, ExamplePart, Ident, Item, Key, Member, MemberKind, MetaType, Metadata,
    NumLit, OpEntry, Operation, Resource, ResourceEntry, ResourceEntryKind, TypeRef, Value,
};
use crate::compile::{FileDiagnostic, Location, Naming, Package, SourceMap};
use crate::diagnostic::Diagnostic;
use crate::literal::resolve_enum_symbols;
use crate::span::Span;

mod permissions;

/// `namespaces[i]` is the namespace of `parsed[i]`: the package's own, or a
/// bundled import's.
pub(crate) struct Lowered {
    pub service: Option<Service>,
    pub source_map: SourceMap,
    pub diagnostics: Vec<FileDiagnostic>,
    /// Types named outside any shape's members: metadata values and resource
    /// entries. Rules count them as referenced.
    pub named_types: BTreeSet<ShapeId>,
    /// Members a role template added to an operation.
    pub template_members: BTreeSet<ShapeId>,
}

pub(crate) fn lower(
    package: &Package,
    parsed: &[Parsed],
    namespaces: &[String],
    service: Option<&str>,
) -> Lowered {
    let mut lowerer = Lowerer {
        ns: &package.namespace,
        naming: &package.naming,
        decls: BTreeMap::new(),
        bodies: BTreeMap::new(),
        diagnostics: Vec::new(),
        source_map: SourceMap::new(),
        protocol: Protocol::CrucibleJson,
        service_name: String::new(),
        shapes: BTreeMap::new(),
        meta_shapes: BTreeMap::new(),
        visiting: BTreeSet::new(),
        pending: BTreeMap::new(),
        requirements: BTreeSet::new(),
        http_inputs: BTreeSet::new(),
        http_outputs: BTreeSet::new(),
        unplaced: BTreeMap::new(),
        named_types: BTreeSet::new(),
        declared_permissions: BTreeMap::new(),
        permissions: BTreeMap::new(),
        permission_facts: crucible_smithy::validate::PermissionFacts::default(),
        service_findings: Vec::new(),
        unlowered_requirements: BTreeSet::new(),
        not_found: None,
        permission_sites: BTreeMap::new(),
        requirement_sites: BTreeMap::new(),
    };
    lowerer.collect(parsed, namespaces);
    lowerer.check_resources();
    lowerer.check_lifecycle();
    lowerer.check_restated_resources();
    lowerer.check_metadata();
    let service = lowerer.service(service);
    Lowered {
        service,
        source_map: lowerer.source_map,
        diagnostics: lowerer.diagnostics,
        named_types: lowerer.named_types,
        template_members: BTreeSet::new(),
    }
}

/// A shape being lowered, as its referrers see it (`Lowerer::pending`).
#[derive(Clone)]
enum Pending {
    Structure,
    Union,
    List(Constraints),
    Map(Constraints),
}

const UNION_DEFAULT: &str = "a union has no default: it has no value until one arm is set";
const STRUCTURE_DEFAULT: &str = "a structure has no default; make the member optional";

#[derive(Clone, Copy)]
struct DeclRef<'a> {
    file: usize,
    item: &'a Item,
    /// The namespace of the package that declares it.
    ns: &'a str,
}

/// An operation's anonymous input or output body, lowered as a structure.
#[derive(Clone)]
struct BodySource<'a> {
    file: usize,
    operation: &'a Operation,
    body: &'a Body,
    /// The operation's URI labels: input members of these names bind to them.
    labels: Vec<String>,
}

/// What a member is lowered inside of.
#[derive(Clone, Copy)]
struct MemberContext<'a> {
    owner: &'a ShapeId,
    /// The resource whose identifiers and properties `$name` may elide.
    resource: Option<&'a str>,
    /// The other resources of a `for A, B` view: what the view shows must be
    /// declared identically by each of them (`resource` is the first).
    also: &'a [&'a str],
    union: bool,
    /// URI labels, for an operation input: a member of that name binds to it.
    labels: &'a [String],
    /// Where the members travel, which decides the bindings they may carry.
    site: Site,
    /// The members come from an `include`, so a misplaced binding is the
    /// includer's doing as much as the member's.
    included: bool,
    /// The input of a collection operation (`creates`, `lists`,
    /// `on .. collection`), which cannot bind the resource's own
    /// identifiers (decision 32).
    collection: bool,
    /// The role of the operation whose input or output this is, while the
    /// members elide against that operation's resource.
    role: Option<&'a str>,
    /// The members are a `for` view's, wherever it is used: they are a
    /// representation of the resource (decisions 34d-34f).
    view: bool,
}

/// What a structure is on the wire.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Site {
    Input,
    Output,
    Error,
    /// A union: nested in a body, so never bound.
    Nested,
    /// A declared structure. Where a member uses it as its type it is nested
    /// and never bound; where it is only included, its members land in the
    /// includer and their bindings are judged there (decision 40). Which of
    /// the two it is is known only once the whole package is lowered, so
    /// its binding problems wait in `Lowerer::unplaced` until then.
    Structure,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Service,
    Operation,
    /// A shape, named by its declaration keyword.
    Shape(&'static str),
    Member,
    EnumValue,
}

impl Position {
    fn target(self) -> &'static str {
        match self {
            Position::Service => "service",
            Position::Operation => "operation",
            Position::Shape(kind) => kind,
            Position::Member => "member",
            Position::EnumValue => "enum value",
        }
    }
}

/// What a declaration's annotations contribute.
#[derive(Default)]
struct Annotations {
    traits: Traits,
    notes: DocNotes,
    /// `@query` with its optional parameter name.
    query: Option<Option<String>>,
    header: Option<String>,
    payload: bool,
    token: bool,
    /// `@mutability(..)`: an explicit override of the derived mutability.
    mutability: Option<Mutability>,
}

/// A resource entry `$name` resolved to, and whether it is the resource's
/// own or an inherited identifier.
#[derive(Clone, Copy)]
struct EntryRef<'a> {
    file: usize,
    entry: &'a ResourceEntry,
    own: bool,
}

/// Whether `structure` is a `for` view that covers `resource`, as its first
/// or one of its other resources.
fn view_of(structure: &crate::ast::Structure, resource: &str) -> bool {
    structure
        .resource
        .iter()
        .chain(&structure.also)
        .any(|r| r.name == resource)
}

/// The fields in which two lowerings of one view member disagree, by name.
/// Documentation is the first resource's, so it is not compared.
fn member_differences(a: &IrMember, b: &IrMember) -> Vec<String> {
    fn fields(member: &IrMember) -> serde_json::Map<String, Json> {
        let mut member = member.clone();
        member.docs = None;
        member.traits.remove(&smithy("documentation"));
        match serde_json::to_value(member) {
            Ok(Json::Object(map)) => map,
            _ => serde_json::Map::new(),
        }
    }
    let (a, b) = (fields(a), fields(b));
    a.iter()
        .filter(|(key, value)| b.get(*key) != Some(*value))
        .map(|(key, _)| key.replace('_', " "))
        .collect()
}

/// A property's declared mutability (decision 34a); an identifier has
/// none. Combinations `check_resources` refuses lower as their first
/// keyword would.
fn entry_mutability(entry: &ResourceEntry) -> Option<Mutability> {
    if entry.kind == ResourceEntryKind::Id {
        return None;
    }
    let has = |word: &str| entry.modifiers.iter().any(|m| m.name == word);
    Some(
        match (has("readonly"), has("immutable"), has("writeonly")) {
            (true, _, _) => Mutability::Read,
            (false, true, true) => Mutability::Create,
            (false, true, false) => Mutability::CreateAndRead,
            (false, false, true) => Mutability::Write,
            (false, false, false) => Mutability::Full,
        },
    )
}

/// How the source declares a mutability on a property.
fn mutability_keyword(mutability: Mutability) -> &'static str {
    match mutability {
        Mutability::Full => "read-write (no keyword)",
        Mutability::Read => "`readonly`",
        Mutability::CreateAndRead => "`immutable`",
        Mutability::Write => "`writeonly`",
        Mutability::Create => "`immutable writeonly`",
    }
}

/// The `@mutability(..)` spelling of a mutability.
fn mutability_annotation(mutability: Mutability) -> &'static str {
    match mutability {
        Mutability::Full => "full",
        Mutability::Read => "read",
        Mutability::CreateAndRead => "createAndRead",
        Mutability::Write => "write",
        Mutability::Create => "create",
    }
}

/// The roles whose input writes the resource's state (decision 34a).
const WRITES: &[&str] = &["creates", "puts", "updates"];

#[derive(Default)]
struct ConstraintInfo {
    constraints: Constraints,
    traits: Traits,
    format: Option<TimestampFormat>,
    media_type: Option<String>,
}

const ROLES: &[&str] = &[
    "creates", "puts", "reads", "updates", "deletes", "lists", "on",
];

/// The roles a resource's lifecycle has one of each (decision 32).
const LIFECYCLE: &[&str] = &["creates", "puts", "reads", "updates", "deletes", "lists"];

fn smithy(name: &str) -> ShapeId {
    ShapeId::new("smithy.api", name)
}

/// Built-in scalar type names and the Smithy prelude shape each one is.
fn prelude(word: &str) -> Option<(&'static str, SimpleKind)> {
    Some(match word {
        "string" => ("String", SimpleKind::String),
        "boolean" => ("Boolean", SimpleKind::Boolean),
        "byte" => ("Byte", SimpleKind::Byte),
        "short" => ("Short", SimpleKind::Short),
        "integer" => ("Integer", SimpleKind::Integer),
        "long" => ("Long", SimpleKind::Long),
        "float" => ("Float", SimpleKind::Float),
        "double" => ("Double", SimpleKind::Double),
        "bigInteger" => ("BigInteger", SimpleKind::BigInteger),
        "bigDecimal" => ("BigDecimal", SimpleKind::BigDecimal),
        "timestamp" => ("Timestamp", SimpleKind::Timestamp),
        "blob" => ("Blob", SimpleKind::Blob),
        "document" => ("Document", SimpleKind::Document),
        _ => return None,
    })
}

fn prelude_kind(name: &str) -> Option<SimpleKind> {
    [
        ("String", SimpleKind::String),
        ("Boolean", SimpleKind::Boolean),
        ("Byte", SimpleKind::Byte),
        ("Short", SimpleKind::Short),
        ("Integer", SimpleKind::Integer),
        ("Long", SimpleKind::Long),
        ("Float", SimpleKind::Float),
        ("Double", SimpleKind::Double),
        ("BigInteger", SimpleKind::BigInteger),
        ("BigDecimal", SimpleKind::BigDecimal),
        ("Timestamp", SimpleKind::Timestamp),
        ("Blob", SimpleKind::Blob),
        ("Document", SimpleKind::Document),
    ]
    .into_iter()
    .find_map(|(prelude, kind)| (prelude == name).then_some(kind))
}

fn decl_keyword(decl: &Decl) -> &'static str {
    match decl {
        Decl::Service(_) => "service",
        Decl::Protocol(_) => "protocol",
        Decl::Resource(_) => "resource",
        Decl::Operation(_) => "operation",
        Decl::Template(_) => "template",
        Decl::Structure(_) => "structure",
        Decl::Union(_) => "union",
        Decl::Error(_) => "error",
        Decl::Enum(_) => "enum",
        Decl::Scalar(_) => "scalar",
        Decl::List(_) => "list",
        Decl::Map(_) => "map",
        Decl::Metadata(_) => "metadata",
        Decl::Rule(_) => "rule",
        Decl::Except(_) => "except",
        Decl::Migration(_) => "migration",
        Decl::Conversion(_) => "conversion",
    }
}

fn decl_name(decl: &Decl) -> Option<&Ident> {
    Some(match decl {
        Decl::Service(block) | Decl::Protocol(block) => &block.name,
        Decl::Resource(r) => &r.name,
        Decl::Operation(o) => &o.name,
        Decl::Structure(s) => &s.name,
        Decl::Union(u) => &u.name,
        Decl::Error(e) => &e.name,
        Decl::Enum(e) => &e.name,
        Decl::Scalar(s) => &s.name,
        Decl::List(l) => &l.name,
        Decl::Map(m) => &m.name,
        Decl::Metadata(m) => &m.name,
        Decl::Rule(r) => &r.name,
        Decl::Template(_) | Decl::Except(_) | Decl::Migration(_) | Decl::Conversion(_) => {
            return None;
        }
    })
}

fn is_type(decl: &Decl) -> bool {
    matches!(
        decl,
        Decl::Structure(_)
            | Decl::Union(_)
            | Decl::Error(_)
            | Decl::Enum(_)
            | Decl::Scalar(_)
            | Decl::List(_)
            | Decl::Map(_)
    )
}

fn doc_text(doc: Option<&Doc>) -> Option<String> {
    doc.map(|doc| doc.text.clone())
}

struct Lowerer<'a> {
    ns: &'a str,
    naming: &'a Naming,
    decls: BTreeMap<&'a str, DeclRef<'a>>,
    bodies: BTreeMap<ShapeId, BodySource<'a>>,
    diagnostics: Vec<FileDiagnostic>,
    source_map: SourceMap,
    protocol: Protocol,
    service_name: String,
    shapes: BTreeMap<ShapeId, Shape>,
    /// Metadata value types, lowered apart from the service's shapes.
    meta_shapes: BTreeMap<ShapeId, Shape>,
    visiting: BTreeSet<ShapeId>,
    /// What a member may need of a shape still being lowered: one it
    /// reaches again through its own members (recursion). A structure or
    /// union has no constraints to give; a list or map has its own.
    pending: BTreeMap<ShapeId, Pending>,
    requirements: BTreeSet<FeatureUse>,
    http_inputs: BTreeSet<ShapeId>,
    http_outputs: BTreeSet<ShapeId>,
    /// Binding problems found in declared structures, by structure: reported
    /// only for a structure something uses as a type (`Site::Structure`).
    unplaced: BTreeMap<ShapeId, Vec<FileDiagnostic>>,
    /// Types named outside any shape's members: metadata value types and
    /// resource identifier and property types.
    named_types: BTreeSet<ShapeId>,
    /// Every resource's permission entries, by resource, whether or not the
    /// service reaches it.
    declared_permissions: BTreeMap<ShapeId, crucible_smithy::ir::ResourcePermissions>,
    /// The service's: its reachable resources' `declared_permissions`.
    permissions: BTreeMap<ShapeId, crucible_smithy::ir::ResourcePermissions>,
    permission_facts: crucible_smithy::validate::PermissionFacts,
    /// What the shared check found in the service, so the check of what it
    /// does not reach repeats none of it.
    service_findings: Vec<crucible_smithy::validate::PermissionFinding>,
    /// Operations whose every requirement clause failed to lower: they are
    /// `Undeclared` in the IR, and not reported as declaring nothing.
    unlowered_requirements: BTreeSet<ShapeId>,
    /// The service's own not-found error (`notFound X`), which check 17
    /// attaches instead of `crucible.errors#NotFoundException`.
    not_found: Option<ShapeId>,
    /// Where resources, their permissions and `@permissionTypes` were
    /// declared, for the shared check's findings.
    permission_sites: BTreeMap<ShapeId, Location>,
    /// Each operation's clauses, by operation and index in its list.
    requirement_sites: BTreeMap<(ShapeId, usize), Location>,
}

impl<'a> Lowerer<'a> {
    // ---- bookkeeping -------------------------------------------------------

    fn error(&mut self, file: usize, span: Span, message: impl Into<String>) {
        self.diagnostics.push(FileDiagnostic {
            file: Some(file),
            diagnostic: Diagnostic::new(span, message),
        });
    }

    fn warning(&mut self, file: usize, span: Span, message: impl Into<String>) {
        self.diagnostics.push(FileDiagnostic {
            file: Some(file),
            diagnostic: Diagnostic::warning(span, message),
        });
    }

    fn package_error(&mut self, message: impl Into<String>) {
        self.diagnostics.push(FileDiagnostic {
            file: None,
            diagnostic: Diagnostic::new(Span::default(), message),
        });
    }

    /// A diagnostic against an IR id, at the source that declared it: the
    /// member, else its shape, else the package.
    fn error_at(&mut self, id: &ShapeId, message: &str) {
        let location = self
            .source_map
            .get(id)
            .or_else(|| self.source_map.get(&id.root()))
            .copied();
        match location {
            Some(Location { file, span }) => self.error(file, span, message),
            None => self.package_error(format!("{id}: {message}")),
        }
    }

    /// A doc comment's text, after warning about `[Name]` links that name
    /// nothing: documentation generators would render them dangling.
    fn doc(&mut self, file: usize, doc: Option<&Doc>) -> Option<String> {
        let doc = doc?;
        for name in doc_links(&doc.text) {
            if self.decl(name).is_none() && prelude(name).is_none() {
                self.warning(
                    file,
                    doc.span,
                    format!("the doc link `[{name}]` names nothing in this package"),
                );
            }
        }
        Some(doc.text.clone())
    }

    fn id(&self, name: &str) -> ShapeId {
        ShapeId::new(self.ns, name)
    }

    /// The id of a declared name, in the namespace of the package that
    /// declares it (the package's own, or an import's).
    fn ref_id(&self, name: &str) -> ShapeId {
        match self.decls.get(name) {
            Some(decl) => ShapeId::new(decl.ns, name),
            None => self.id(name),
        }
    }

    /// Records where an IR shape, member or operation was declared.
    fn locate(&mut self, id: ShapeId, file: usize, span: Span) {
        self.source_map.insert(id, Location { file, span });
    }

    fn decl(&self, name: &str) -> Option<DeclRef<'a>> {
        self.decls.get(name).copied()
    }

    fn require(&mut self, shape: &ShapeId, feature: Feature, source: ShapeId) {
        self.requirements.insert(FeatureUse {
            feature,
            shape: shape.clone(),
            source,
        });
    }

    fn collect(&mut self, parsed: &'a [Parsed], namespaces: &'a [String]) {
        for (file, parsed) in parsed.iter().enumerate() {
            let ns = namespaces.get(file).map_or(self.ns, String::as_str);
            for item in &parsed.file.items {
                match &item.decl {
                    // Migrations are checked against the baseline by `guard`.
                    Decl::Migration(_) => continue,
                    // Checked and run apart from the model (`crate::rules`).
                    Decl::Rule(_) | Decl::Except(_) => continue,
                    Decl::Template(_) | Decl::Protocol(_) | Decl::Conversion(_) => {
                        let keyword = decl_keyword(&item.decl);
                        self.error(
                            file,
                            item.span,
                            format!(
                                "`{keyword}` declarations are not supported by the compiler yet"
                            ),
                        );
                        continue;
                    }
                    _ => {}
                }
                let Some(name) = decl_name(&item.decl) else {
                    continue;
                };
                if prelude(&name.name).is_some() {
                    self.error(
                        file,
                        name.span,
                        format!("`{}` is a built-in type and cannot be declared", name.name),
                    );
                    continue;
                }
                if name.name.contains('-') {
                    self.error(
                        file,
                        name.span,
                        format!(
                            "a name is an identifier: `{}` cannot contain `-`",
                            name.name
                        ),
                    );
                }
                if let Some(previous) = self.decls.get(name.name.as_str()).copied() {
                    let kind = decl_keyword(&previous.item.decl);
                    if previous.ns == ns {
                        self.error(
                            file,
                            name.span,
                            format!(
                                "`{}` is already declared as {} {kind}",
                                name.name,
                                article(kind)
                            ),
                        );
                    } else {
                        // The package's own files come first, so `previous` is
                        // the package's declaration and this one the import's:
                        // report it where the author can rename it.
                        let span =
                            decl_name(&previous.item.decl).map_or(previous.item.span, |n| n.span);
                        self.error(
                            previous.file,
                            span,
                            format!(
                                "`{}` is also declared in the imported package {ns}; rename it",
                                name.name
                            ),
                        );
                    }
                    continue;
                }
                self.decls.insert(&name.name, DeclRef { file, item, ns });
            }
        }
    }

    /// The identifiers of `resource`: its parents' first, then its own.
    fn identifiers(&self, resource: &str) -> (Vec<String>, Vec<String>) {
        let mut chain = Vec::new();
        let mut current = Some(resource.to_owned());
        let mut seen = BTreeSet::new();
        while let Some(name) = current {
            if !seen.insert(name.clone()) {
                break;
            }
            let Some(Decl::Resource(r)) = self.decl(&name).map(|d| &d.item.decl) else {
                break;
            };
            chain.push(r);
            current = r.parent.as_ref().map(|p| p.name.clone());
        }
        let ids = |r: &Resource| -> Vec<String> {
            r.entries
                .iter()
                .filter(|e| e.kind == ResourceEntryKind::Id)
                .map(|e| e.name.name.clone())
                .collect()
        };
        let own = chain.first().map(|r| ids(r)).unwrap_or_default();
        let inherited = chain.iter().skip(1).rev().flat_map(|r| ids(r)).collect();
        (inherited, own)
    }

    /// Decision 31: what a resource declares, checked once whether or not a
    /// service reaches it; and decision 32's one operation per lifecycle role.
    fn check_resources(&mut self) {
        let decls: Vec<DeclRef<'a>> = self.decls.values().copied().collect();
        for decl in &decls {
            let Decl::Resource(resource) = &decl.item.decl else {
                continue;
            };
            let file = decl.file;
            self.modifiers(file, decl.item, &[]);
            if let Some(annotation) = decl
                .item
                .annotations
                .iter()
                .find(|a| !permissions::resource_annotation(a))
            {
                self.error(
                    file,
                    annotation.span,
                    "annotations on a resource are not supported by the compiler yet, apart from `@authorizationName`",
                );
            }
            self.declare_permissions(*decl, resource);
            if let Some(parent) = &resource.parent {
                match self.decl(&parent.name).map(|d| &d.item.decl) {
                    Some(Decl::Resource(_)) => {}
                    _ => self.error(
                        file,
                        parent.span,
                        format!("`{}` is not a resource", parent.name),
                    ),
                }
                // Follow the chain: reaching this resource again is a cycle.
                let mut current = Some(parent.name.clone());
                let mut seen = BTreeSet::new();
                while let Some(name) = current {
                    if name == resource.name.name {
                        self.error(
                            file,
                            resource.name.span,
                            format!("`{}` is its own ancestor", resource.name.name),
                        );
                        break;
                    }
                    if !seen.insert(name.clone()) {
                        break;
                    }
                    current = match self.decl(&name).map(|d| &d.item.decl) {
                        Some(Decl::Resource(r)) => r.parent.as_ref().map(|p| p.name.clone()),
                        _ => None,
                    };
                }
            }
            self.doc(file, decl.item.doc.as_ref());
            // Identifier names are unique across the chain, since a child
            // binds every inherited one; a property belongs to its own
            // resource and only must not shadow an identifier (decision 31).
            let (inherited, _) = self.identifiers(&resource.name.name);
            let mut names = BTreeSet::new();
            for entry in &resource.entries {
                self.doc(file, entry.doc.as_ref());
                if let Some(annotation) = entry.annotations.first() {
                    self.error(
                        file,
                        annotation.span,
                        "annotations on a resource identifier or property are not supported by the compiler yet",
                    );
                }
                self.entry_keywords(file, entry);
                if !names.insert(entry.name.name.as_str()) {
                    self.error(
                        file,
                        entry.name.span,
                        format!("`{}` is declared twice", entry.name.name),
                    );
                } else if inherited.contains(&entry.name.name) {
                    let message = match entry.kind {
                        ResourceEntryKind::Id => format!(
                            "`{}` is already declared by an ancestor of `{}`",
                            entry.name.name, resource.name.name
                        ),
                        ResourceEntryKind::Property => format!(
                            "`{}` is an identifier `{}` inherits; a property cannot share its name",
                            entry.name.name, resource.name.name
                        ),
                    };
                    self.error(file, entry.name.span, message);
                }
            }
            self.representation_names(file, resource, &inherited);
            for entry in &resource.entries {
                let Some(target) = self.resolve_type(file, &entry.ty.name) else {
                    continue;
                };
                self.named_types.insert(target.clone());
                if entry.kind == ResourceEntryKind::Id {
                    let text = target == smithy("String")
                        || match self.decl(target.name()).map(|d| &d.item.decl) {
                            Some(Decl::Scalar(s)) => s.kind.name == "string",
                            Some(Decl::Enum(e)) => e.base.is_none(),
                            _ => false,
                        };
                    if !text {
                        self.error(
                            file,
                            entry.ty.span,
                            "a resource identifier is a string or a string enum: it is part of a URI",
                        );
                    }
                }
            }
        }
        // Each lifecycle role is held by one operation per resource.
        let mut holders: BTreeMap<(String, String), String> = BTreeMap::new();
        for decl in &decls {
            let Decl::Operation(operation) = &decl.item.decl else {
                continue;
            };
            let Some(attachment) = &operation.attachment else {
                continue;
            };
            let role = attachment.role.name.as_str();
            if !LIFECYCLE.contains(&role) {
                continue;
            }
            let key = (attachment.resource.name.clone(), role.to_owned());
            if let Some(holder) = holders.get(&key) {
                self.error(
                    decl.file,
                    attachment.span,
                    format!(
                        "`{}` already has a `{role}` operation, `{holder}`; each lifecycle role is held by one operation",
                        attachment.resource.name
                    ),
                );
            } else {
                holders.insert(key, operation.name.name.clone());
            }
        }
    }

    /// The operations of this package attached to `resource`, with their
    /// attachments.
    fn attached(&self, resource: &str) -> Vec<(&'a Operation, &'a Attachment)> {
        self.decls
            .values()
            .filter(|decl| decl.ns == self.ns)
            .filter_map(|decl| match &decl.item.decl {
                Decl::Operation(operation) => operation
                    .attachment
                    .as_ref()
                    .filter(|a| a.resource.name == resource)
                    .map(|a| (operation, a)),
                _ => None,
            })
            .collect()
    }

    /// The body of `kind` an operation declares, if any.
    fn body(operation: &'a Operation, kind: BodyKind) -> Option<&'a Body> {
        operation.entries.iter().find_map(|entry| match entry {
            OpEntry::Body(body) if body.kind == kind => Some(body),
            _ => None,
        })
    }

    /// The names of `resource`'s own properties that `members` write:
    /// elisions not under `@query`, through `include`s of structures for no
    /// other resource. Mirrors which bindings `bound_mutability` treats as
    /// writes.
    fn written(
        &self,
        members: &'a [Member],
        resource: &str,
        including: &mut Vec<&'a str>,
        out: &mut BTreeSet<&'a str>,
    ) {
        for member in members {
            match &member.kind {
                MemberKind::Elided { name, .. }
                    if !member.annotations.iter().any(|a| a.name.name == "query") =>
                {
                    if let Some(found) = self.resource_entry(resource, &name.name)
                        && found.own
                        && found.entry.kind == ResourceEntryKind::Property
                    {
                        out.insert(found.entry.name.name.as_str());
                    }
                }
                MemberKind::Include(name) if !including.contains(&name.name.as_str()) => {
                    if let Some(Decl::Structure(structure)) =
                        self.decl(&name.name).map(|d| &d.item.decl)
                        && structure
                            .resource
                            .as_ref()
                            .is_none_or(|r| r.name == resource)
                    {
                        including.push(&name.name);
                        self.written(&structure.members, resource, including, out);
                        including.pop();
                    }
                }
                _ => {}
            }
        }
    }

    /// Whether `members`, elided against `resource`, show one of its
    /// properties: an elision, `$*` over a readable property, an included
    /// view, or a member typed by a `for` view of it (or a list or map of
    /// one). Decision 34h's first warning counts these.
    fn shows_property(
        &self,
        members: &'a [Member],
        resource: &str,
        seen: &mut BTreeSet<&'a str>,
    ) -> bool {
        members.iter().any(|member| match &member.kind {
            MemberKind::Elided { name, .. } => self
                .resource_entry(resource, &name.name)
                .is_some_and(|found| found.own && found.entry.kind == ResourceEntryKind::Property),
            MemberKind::All(_) => self.chain(resource).first().is_some_and(|(_, r)| {
                r.entries.iter().any(|e| {
                    e.kind == ResourceEntryKind::Property
                        && !e.modifiers.iter().any(|m| m.name == "writeonly")
                })
            }),
            MemberKind::Include(name) => self.view_shows_property(&name.name, resource, seen, true),
            MemberKind::Field { ty: Some(ty), .. } => {
                self.view_shows_property(&ty.name.name, resource, seen, false)
            }
            MemberKind::Field { ty: None, .. } => false,
        })
    }

    /// Whether the type `name` is (a list or map of) a `for resource` view
    /// that shows one of its properties. An included plain structure counts
    /// too, since its members land where it is included.
    fn view_shows_property(
        &self,
        name: &'a str,
        resource: &str,
        seen: &mut BTreeSet<&'a str>,
        included: bool,
    ) -> bool {
        if !seen.insert(name) {
            return false;
        }
        match self.decl(name).map(|d| &d.item.decl) {
            Some(Decl::Structure(structure)) => match &structure.resource {
                Some(_) if view_of(structure, resource) => {
                    self.shows_property(&structure.members, resource, seen)
                }
                None if included => self.shows_property(&structure.members, resource, seen),
                _ => false,
            },
            Some(Decl::List(list)) if !included => {
                self.view_shows_property(&list.member.name, resource, seen, false)
            }
            Some(Decl::Map(map)) if !included => {
                self.view_shows_property(&map.value.name, resource, seen, false)
            }
            _ => false,
        }
    }

    /// Decisions 34a and 34h, per resource of this package: an `immutable`
    /// property something must set, a property nothing updates that is left
    /// read-write, and a resource whose outputs show none of its properties.
    fn check_lifecycle(&mut self) {
        let decls: Vec<DeclRef<'a>> = self
            .decls
            .values()
            .filter(|decl| decl.ns == self.ns)
            .copied()
            .collect();
        for decl in decls {
            let Decl::Resource(resource) = &decl.item.decl else {
                continue;
            };
            let file = decl.file;
            let name = resource.name.name.as_str();
            let operations = self.attached(name);
            let mut created = BTreeSet::new();
            let mut updated = BTreeSet::new();
            for (operation, attachment) in &operations {
                let Some(input) = Self::body(operation, BodyKind::Input) else {
                    continue;
                };
                match (attachment.role.name.as_str(), attachment.collection) {
                    // A custom collection action makes new instances, as
                    // Pigeon's `CreateApiKey` does (34g), so what it binds
                    // is set at creation.
                    ("creates", _) | ("on", true) => {
                        self.written(&input.members, name, &mut Vec::new(), &mut created)
                    }
                    // A custom instance action's input is not checked
                    // against the keywords, but what it binds does change
                    // after creation (Leyline's `PutGraphGroupMembership`
                    // moves a graph's `groupId`), so it counts as a change.
                    ("updates", _) | ("on", false) => {
                        self.written(&input.members, name, &mut Vec::new(), &mut updated)
                    }
                    ("puts", _) => {
                        let mut put = BTreeSet::new();
                        self.written(&input.members, name, &mut Vec::new(), &mut put);
                        created.extend(put.iter().copied());
                        updated.extend(put);
                    }
                    _ => {}
                }
            }
            for entry in &resource.entries {
                let property = entry.name.name.as_str();
                let Some(mutability) = entry_mutability(entry) else {
                    continue;
                };
                if property.is_empty() {
                    continue;
                }
                match mutability {
                    Mutability::CreateAndRead | Mutability::Create
                        if !created.contains(property) =>
                    {
                        self.error(
                            file,
                            entry.name.span,
                            format!(
                                "`{property}` is {}, but no `creates`, `puts` or `on .. collection` input binds `${property}`: nothing could ever set it",
                                mutability_keyword(mutability)
                            ),
                        );
                    }
                    Mutability::Full | Mutability::Write if !updated.contains(property) => {
                        // A `property id` repeating an identifier's value is
                        // the identifier's spelling in views (34f). Only the
                        // name `id`: two entries may share a plain string
                        // type without being one value.
                        let repeated = resource.entries.iter().find(|id| {
                            property == "id"
                                && id.kind == ResourceEntryKind::Id
                                && id.alias.is_none()
                                && self.type_id(&id.ty.name.name).is_some()
                                && self.type_id(&id.ty.name.name)
                                    == self.type_id(&entry.ty.name.name)
                        });
                        let message = if let Some(id) = repeated
                            && !created.contains(property)
                        {
                            format!(
                                "`{property}` repeats the identifier `{0}`, which nothing writes: declare `id {0}: {1} as {property}` and remove the property, so views show the identifier as `{property}`",
                                id.name.name, id.ty.name.name
                            )
                        } else if created.contains(property) {
                            let keyword = if mutability == Mutability::Write {
                                "immutable writeonly"
                            } else {
                                "immutable"
                            };
                            format!(
                                "no `updates`, `puts` or `on` input binds `${property}`, so nothing changes it after creation: declare it `{keyword}`, or infrastructure tools will offer an update nothing implements"
                            )
                        } else if mutability == Mutability::Write {
                            format!(
                                "no `creates`, `puts`, `updates` or `on` input binds `${property}`: a `writeonly` property nothing writes is never set nor read"
                            )
                        } else {
                            format!(
                                "no `creates`, `puts`, `updates` or `on` input binds `${property}`, so the service alone sets it: declare it `readonly`"
                            )
                        };
                        self.warning(file, entry.name.span, message);
                    }
                    _ => {}
                }
            }
            let shown_by: Vec<&str> = operations
                .iter()
                .map(|(_, attachment)| attachment.role.name.as_str())
                .filter(|role| matches!(*role, "reads" | "lists" | "creates"))
                .collect();
            if shown_by.is_empty() {
                continue;
            }
            let shown = operations.iter().any(|(operation, _)| {
                Self::body(operation, BodyKind::Output).is_some_and(|output| {
                    self.shows_property(&output.members, name, &mut BTreeSet::new())
                })
            });
            if !shown {
                let mut roles: Vec<&str> = shown_by;
                roles.sort_unstable();
                roles.dedup();
                let mut roles: Vec<String> = roles.iter().map(|r| format!("`{r}`")).collect();
                let last = roles.pop().unwrap_or_default();
                let roles = if roles.is_empty() {
                    last
                } else {
                    format!("{} and {last}", roles.join(", "))
                };
                self.warning(
                    file,
                    resource.name.span,
                    format!(
                        "`{name}` has {roles} operations, but no output shows one of its properties, through a `for {name}` view or a `$` elision: a representation written as a plain structure loses `$` binding, mutability and infrastructure lowering"
                    ),
                );
            }
        }
    }

    /// Decision 34h's second warning: a structure declared without `for`
    /// that restates, by name and type, an identifier and a property of one
    /// resource of this package.
    fn check_restated_resources(&mut self) {
        let decls: Vec<DeclRef<'a>> = self
            .decls
            .values()
            .filter(|decl| decl.ns == self.ns)
            .copied()
            .collect();
        let resources: Vec<&'a Resource> = decls
            .iter()
            .filter_map(|decl| match &decl.item.decl {
                Decl::Resource(r) => Some(r),
                _ => None,
            })
            .collect();
        for decl in &decls {
            let Decl::Structure(structure) = &decl.item.decl else {
                continue;
            };
            if structure.resource.is_some() {
                continue;
            }
            let fields: Vec<(&str, Option<ShapeId>)> = structure
                .members
                .iter()
                .filter_map(|member| match &member.kind {
                    MemberKind::Field {
                        name, ty: Some(ty), ..
                    } => Some((name.name.as_str(), self.type_id(&ty.name.name))),
                    _ => None,
                })
                .collect();
            let restates = |entry: &ResourceEntry, spelled: &str| {
                let ty = self.type_id(&entry.ty.name.name);
                ty.is_some()
                    && fields
                        .iter()
                        .any(|(name, field)| *name == spelled && *field == ty)
            };
            let found = resources.iter().find_map(|resource| {
                let id = resource.entries.iter().find(|e| {
                    e.kind == ResourceEntryKind::Id
                        && (restates(e, &e.name.name)
                            || e.alias.as_ref().is_some_and(|a| restates(e, &a.name)))
                })?;
                let property = resource
                    .entries
                    .iter()
                    .find(|e| e.kind == ResourceEntryKind::Property && restates(e, &e.name.name))?;
                Some((resource.name.name.as_str(), id, property))
            });
            if let Some((resource, id, property)) = found {
                self.warning(
                    decl.file,
                    structure.name.span,
                    format!(
                        "`{0}` restates `{resource}`'s identifier `{1}` and property `{2}`: did you mean `structure {0} for {resource}`, whose `$` entries take their types and mutability from the resource?",
                        structure.name.name, id.name.name, property.name.name
                    ),
                );
            }
        }
    }

    /// Decisions 34a, 34e and 34f: what an entry's keywords, `?` and `as`
    /// may say.
    fn entry_keywords(&mut self, file: usize, entry: &ResourceEntry) {
        match entry.kind {
            ResourceEntryKind::Id => {
                if let Some(modifier) = entry.modifiers.first() {
                    self.error(
                        file,
                        modifier.span,
                        format!(
                            "an identifier is immutable and takes no keyword; `{}` applies to a property",
                            modifier.name
                        ),
                    );
                }
                if entry.optional {
                    self.error(
                        file,
                        entry.name.span,
                        format!(
                            "an identifier is always present: `{}` cannot be optional",
                            entry.name.name
                        ),
                    );
                }
            }
            ResourceEntryKind::Property => {
                let mut given = BTreeSet::new();
                for modifier in &entry.modifiers {
                    if !given.insert(modifier.name.as_str()) {
                        self.error(
                            file,
                            modifier.span,
                            format!("`{}` is given twice", modifier.name),
                        );
                    }
                }
                if given.contains("readonly") && given.len() > 1 {
                    let other = entry
                        .modifiers
                        .iter()
                        .find(|m| m.name != "readonly")
                        .expect("two keywords");
                    self.error(
                        file,
                        other.span,
                        format!(
                            "`readonly` combines with no other keyword: the service assigns `{}`, so no client sets it once or writes it unseen",
                            entry.name.name
                        ),
                    );
                }
                if let Some(alias) = &entry.alias {
                    self.error(
                        file,
                        alias.span,
                        format!(
                            "only an identifier names its spelling in views; a view binds a property under another name with `${} as {}`",
                            entry.name.name, alias.name
                        ),
                    );
                }
            }
        }
    }

    /// Decision 34f: a representation name is unique among the entries the
    /// resource's own views can show, its own entries and its inherited
    /// identifiers. Plain name clashes are reported with the entries; this
    /// reports the clashes an identifier's `as` spelling makes.
    fn representation_names(&mut self, file: usize, resource: &Resource, inherited: &[String]) {
        let mut shown: BTreeMap<String, String> = inherited
            .iter()
            .map(|id| (id.clone(), format!("the inherited identifier `{id}`")))
            .collect();
        for entry in &resource.entries {
            let spelled = entry
                .alias
                .as_ref()
                .filter(|_| entry.kind == ResourceEntryKind::Id);
            if spelled.is_none_or(|alias| alias.name == entry.name.name) {
                let what = match entry.kind {
                    ResourceEntryKind::Id => "identifier",
                    ResourceEntryKind::Property => "property",
                };
                shown
                    .entry(entry.name.name.clone())
                    .or_insert_with(|| format!("the {what} `{}`", entry.name.name));
            }
        }
        for entry in &resource.entries {
            let Some(alias) = &entry.alias else {
                continue;
            };
            if entry.kind != ResourceEntryKind::Id || alias.name == entry.name.name {
                continue;
            }
            match shown.get(&alias.name).cloned() {
                Some(other) => self.error(
                    file,
                    alias.span,
                    format!(
                        "`{}` already names {other} in `{}`'s views; an identifier's spelling is unique among the names its resource's views show",
                        alias.name, resource.name.name
                    ),
                ),
                None => {
                    shown.insert(
                        alias.name.clone(),
                        format!("the identifier `{}`", entry.name.name),
                    );
                }
            }
        }
    }

    /// Decision 32: the role names a known verb, and fixes which identifiers
    /// the input binds and which keywords it implies.
    fn check_role(
        &mut self,
        file: usize,
        operation: &Operation,
        attachment: &Attachment,
        input: Option<&Body>,
        modifiers: &BTreeSet<String>,
    ) {
        let role = attachment.role.name.as_str();
        if !ROLES.contains(&role) {
            self.error(
                file,
                attachment.role.span,
                format!(
                    "`{role}` is not a role; a role is one of {}",
                    ROLES.join(", ")
                ),
            );
            return;
        }
        if attachment.collection && role != "on" {
            self.error(
                file,
                attachment.span,
                format!(
                    "`collection` follows `on` only: `on {} collection`",
                    attachment.resource.name
                ),
            );
        }
        match role {
            "reads" | "lists" if modifiers.contains("idempotent") => self.error(
                file,
                operation.name.span,
                format!("`{role}` implies `readonly`; the operation is not also `idempotent`"),
            ),
            "puts" | "deletes" if modifiers.contains("readonly") => self.error(
                file,
                operation.name.span,
                format!("`{role}` implies `idempotent`; the operation is not also `readonly`"),
            ),
            // Smithy's update lifecycle forbids `readonly` too, though it
            // implies no idempotency.
            "updates" if modifiers.contains("readonly") => self.error(
                file,
                operation.name.span,
                "`updates` changes the resource; the operation is not `readonly`",
            ),
            _ => {}
        }
        if !matches!(
            self.decl(&attachment.resource.name).map(|d| &d.item.decl),
            Some(Decl::Resource(_))
        ) {
            return;
        }
        let bound: BTreeSet<&str> = input
            .map(|body| {
                body.members
                    .iter()
                    .filter_map(|member| match &member.kind {
                        MemberKind::Elided { name, .. } => Some(name.name.as_str()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (inherited, own) = self.identifiers(&attachment.resource.name);
        let collection = matches!(role, "creates" | "lists") || attachment.collection;
        let required: Vec<&String> = if collection {
            inherited.iter().collect()
        } else {
            inherited.iter().chain(&own).collect()
        };
        let missing: Vec<String> = required
            .iter()
            .filter(|id| !bound.contains(id.as_str()))
            .map(|id| format!("`${id}`"))
            .collect();
        if !missing.is_empty() {
            let which = if collection {
                "its parent's identifiers"
            } else {
                "every identifier"
            };
            self.error(
                file,
                attachment.span,
                format!(
                    "`{}` {role} `{}`, so its input binds {which}: add {}",
                    operation.name.name,
                    attachment.resource.name,
                    missing.join(", ")
                ),
            );
        }
        if collection {
            for id in own.iter().filter(|id| bound.contains(id.as_str())) {
                let hint = if role == "creates" {
                    "; an operation that chooses the identifier `puts`"
                } else {
                    ""
                };
                self.error(
                    file,
                    attachment.span,
                    format!(
                        "a collection operation binds only the parent's identifiers, not `${id}`{hint}"
                    ),
                );
            }
        }
    }

    // ---- names and values --------------------------------------------------

    /// The type a name refers to, if it is one; [`Self::resolve_type`] reports
    /// when it is not, this does not.
    fn type_id(&self, name: &str) -> Option<ShapeId> {
        match prelude(name) {
            Some((prelude, _)) => Some(smithy(prelude)),
            None => self
                .decl(name)
                .filter(|decl| is_type(&decl.item.decl))
                .map(|_| self.ref_id(name)),
        }
    }

    fn resolve_type(&mut self, file: usize, name: &Ident) -> Option<ShapeId> {
        if let Some((prelude, _)) = prelude(&name.name) {
            return Some(smithy(prelude));
        }
        match self.decl(&name.name) {
            Some(decl) if is_type(&decl.item.decl) => Some(self.ref_id(&name.name)),
            Some(decl) => {
                let kind = decl_keyword(&decl.item.decl);
                self.error(
                    file,
                    name.span,
                    format!("`{}` is a {kind}, not a type", name.name),
                );
                None
            }
            None => {
                self.error(file, name.span, format!("unknown type `{}`", name.name));
                None
            }
        }
    }

    fn number(&mut self, file: usize, literal: &NumLit) -> Option<Number> {
        match serde_json::from_str::<Number>(&literal.text) {
            Ok(number) => Some(number),
            Err(_) => {
                self.error(
                    file,
                    literal.span,
                    format!("`{}` is not a valid number", literal.text),
                );
                None
            }
        }
    }

    fn json(&mut self, file: usize, value: &Value) -> Json {
        match value {
            Value::Str(s) => Json::String(s.value.clone()),
            Value::Number(n) => self.number(file, n).map_or(Json::Null, Json::Number),
            Value::Ident(ident) => match ident.name.as_str() {
                "true" => Json::Bool(true),
                "false" => Json::Bool(false),
                "null" => Json::Null,
                name => Json::String(name.to_owned()),
            },
            Value::List(items, _) => {
                Json::Array(items.iter().map(|v| self.json(file, v)).collect())
            }
            Value::Object(fields, _) => {
                let mut map = JsonMap::new();
                for field in fields {
                    let (key, span) = match &field.key {
                        Key::Name(name) => (name.name.clone(), name.span),
                        Key::Text(text) => (text.value.clone(), text.span),
                    };
                    let value = self.json(file, &field.value);
                    if map.insert(key.clone(), value).is_some() {
                        self.error(file, span, format!("`{key}` is given twice"));
                    }
                }
                Json::Object(map)
            }
        }
    }

    // ---- annotations, documentation and constraints ------------------------

    fn annotations(
        &mut self,
        file: usize,
        annotations: &[Annotation],
        modifiers: &[Ident],
        position: Position,
    ) -> Annotations {
        let mut out = Annotations::default();
        let mut custom: Vec<(&Ident, &Metadata)> = Vec::new();
        let mut given = BTreeSet::new();
        for annotation in annotations {
            let name = annotation.name.name.as_str();
            if !given.insert(name) {
                self.error(file, annotation.span, format!("`@{name}` is given twice"));
                continue;
            }
            let member_only = matches!(
                name,
                "query" | "header" | "payload" | "token" | "mutability"
            );
            if member_only && position != Position::Member {
                self.error(
                    file,
                    annotation.span,
                    format!("`@{name}` applies to members only"),
                );
                continue;
            }
            match name {
                "deprecated" => {
                    let mut message = None;
                    let mut since = None;
                    for arg in &annotation.args {
                        let key = match &arg.key {
                            Some(Key::Name(key)) => key.name.as_str(),
                            _ => "",
                        };
                        let text = match &arg.value {
                            Value::Str(s) => Some(s.value.clone()),
                            _ => None,
                        };
                        match (key, text) {
                            ("message", Some(text)) => message = Some(text),
                            ("since", Some(text)) => since = Some(text),
                            _ => self.error(
                                file,
                                arg.value.span(),
                                "`@deprecated` takes `message: \"..\"` and `since: \"..\"`",
                            ),
                        }
                    }
                    // Canonical key order, so the trait value does not depend
                    // on the order the arguments were written in.
                    let mut value = JsonMap::new();
                    if let Some(message) = &message {
                        value.insert("message".into(), Json::String(message.clone()));
                    }
                    if let Some(since) = &since {
                        value.insert("since".into(), Json::String(since.clone()));
                    }
                    out.traits.insert(smithy("deprecated"), Json::Object(value));
                    out.notes.deprecated = Some(Deprecation { message, since });
                }
                "since" => match annotation.args.as_slice() {
                    [arg] if arg.key.is_none() && matches!(arg.value, Value::Str(_)) => {
                        let Value::Str(s) = &arg.value else {
                            unreachable!("matched")
                        };
                        out.traits
                            .insert(smithy("since"), Json::String(s.value.clone()));
                        out.notes.since = Some(s.value.clone());
                    }
                    _ => self.error(file, annotation.span, "`@since` takes one string"),
                },
                "unstable" => {
                    self.no_args(file, annotation);
                    out.traits
                        .insert(smithy("unstable"), Json::Object(JsonMap::new()));
                    out.notes.unstable = true;
                }
                "externalDocs" => {
                    let mut value = JsonMap::new();
                    for arg in &annotation.args {
                        match (&arg.key, &arg.value) {
                            (Some(Key::Text(name)), Value::Str(url)) => {
                                value.insert(name.value.clone(), Json::String(url.value.clone()));
                                out.notes
                                    .external_docs
                                    .insert(name.value.clone(), url.value.clone());
                            }
                            _ => self.error(
                                file,
                                arg.value.span(),
                                "`@externalDocs` takes `\"Name\" = \"https://..\"` pairs",
                            ),
                        }
                    }
                    out.traits
                        .insert(smithy("externalDocumentation"), Json::Object(value));
                }
                "query" => match annotation.args.as_slice() {
                    [] => out.query = Some(None),
                    [arg] if arg.key.is_none() && matches!(arg.value, Value::Str(_)) => {
                        let Value::Str(s) = &arg.value else {
                            unreachable!("matched")
                        };
                        out.query = Some(Some(s.value.clone()));
                    }
                    _ => self.error(
                        file,
                        annotation.span,
                        "`@query` takes an optional parameter name",
                    ),
                },
                "header" => match annotation.args.as_slice() {
                    [arg] if arg.key.is_none() && matches!(arg.value, Value::Str(_)) => {
                        let Value::Str(s) = &arg.value else {
                            unreachable!("matched")
                        };
                        out.header = Some(s.value.clone());
                    }
                    _ => self.error(
                        file,
                        annotation.span,
                        "`@header` takes the header name, such as `@header(\"X-Request-Id\")`",
                    ),
                },
                "payload" => {
                    self.no_args(file, annotation);
                    out.payload = true;
                }
                "token" => {
                    self.no_args(file, annotation);
                    out.token = true;
                }
                // Its arguments are read with the service's permissions.
                "permissionTypes" => {
                    if position != Position::Shape("enum") {
                        self.error(
                            file,
                            annotation.span,
                            "`@permissionTypes` applies to an enum: the wire enum of resource types a grant names",
                        );
                    }
                }
                // Read where they apply (`lower::permissions`); anywhere
                // else they are misplaced, never metadata, which cannot
                // take a built-in's name.
                "authorizationName" | "permissionOnly" | "notGrantable" | "aliasOf" => {
                    let target = if name == "authorizationName" {
                        "a resource that declares permissions"
                    } else {
                        "a `permission` entry of a resource"
                    };
                    self.error(
                        file,
                        annotation.span,
                        format!(
                            "`@{name}` applies to {target}, not to a {}",
                            position.target()
                        ),
                    );
                }
                "mutability" => {
                    let value = match annotation.args.as_slice() {
                        [arg] if arg.key.is_none() => match &arg.value {
                            Value::Ident(ident) => match ident.name.as_str() {
                                "full" => Some(Mutability::Full),
                                "create" => Some(Mutability::Create),
                                "createAndRead" => Some(Mutability::CreateAndRead),
                                "read" => Some(Mutability::Read),
                                "write" => Some(Mutability::Write),
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    };
                    match value {
                        Some(value) => out.mutability = Some(value),
                        None => self.error(
                            file,
                            annotation.span,
                            "`@mutability` is one of full, create, createAndRead, read, write",
                        ),
                    }
                }
                _ => {
                    let Some(decl) = self.decl(name) else {
                        self.error(
                            file,
                            annotation.name.span,
                            format!("unknown annotation `@{name}`; declare it with `metadata {name} on {}`", position.target()),
                        );
                        continue;
                    };
                    let Decl::Metadata(metadata) = &decl.item.decl else {
                        self.error(
                            file,
                            annotation.name.span,
                            format!("`{name}` is not a metadata key"),
                        );
                        continue;
                    };
                    let targets: Vec<String> = metadata
                        .targets
                        .iter()
                        .map(|words| {
                            words
                                .iter()
                                .map(|w| w.name.as_str())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .collect();
                    if !targets.iter().any(|target| target == position.target()) {
                        self.error(
                            file,
                            annotation.span,
                            format!(
                                "`@{name}` applies to {}, not to a {}",
                                targets.join(", "),
                                position.target()
                            ),
                        );
                        continue;
                    }
                    let value = self.metadata_value(file, annotation, metadata);
                    // Source-only metadata is for the compiler and rules; the
                    // runtime never sees it (decision 43). It stays in the
                    // IR until the rules have run, and `compile` removes it.
                    out.traits.insert(self.ref_id(name), value);
                    custom.push((&annotation.name, metadata));
                }
            }
        }
        // A key may conflict with another key or with a modifier.
        let present: BTreeSet<&str> = annotations
            .iter()
            .map(|a| a.name.name.as_str())
            .chain(modifiers.iter().map(|m| m.name.as_str()))
            .collect();
        for (name, metadata) in custom {
            for conflict in &metadata.conflicts {
                if present.contains(conflict.name.as_str()) {
                    self.error(
                        file,
                        name.span,
                        format!("`@{}` conflicts with `{}`", name.name, conflict.name),
                    );
                }
            }
        }
        out
    }

    /// Decisions 43 and 44, once per metadata declaration rather than at
    /// every use: targets, conflicts, retention, the value type, and no
    /// shadowing of a built-in annotation, which would always win.
    fn check_metadata(&mut self) {
        const TARGETS: &[&str] = &[
            "service",
            "resource",
            "operation",
            "structure",
            "union",
            "error",
            "enum",
            "scalar",
            "list",
            "map",
            "member",
            "enum value",
        ];
        const BUILT_IN: &[&str] = &[
            "deprecated",
            "since",
            "unstable",
            "externalDocs",
            "query",
            "header",
            "payload",
            "token",
            "mutability",
            "permissionTypes",
            "authorizationName",
            "permissionOnly",
            "notGrantable",
            "aliasOf",
        ];
        const MODIFIERS: &[&str] = &[
            "internal",
            "sensitive",
            "strict",
            "readonly",
            "idempotent",
            "public",
            "sparse",
        ];
        let decls: Vec<DeclRef<'a>> = self.decls.values().copied().collect();
        let mut types = Vec::new();
        for decl in &decls {
            let Decl::Metadata(metadata) = &decl.item.decl else {
                continue;
            };
            let file = decl.file;
            let name = &metadata.name;
            if BUILT_IN.contains(&name.name.as_str()) {
                self.error(
                    file,
                    name.span,
                    format!(
                        "`@{}` is a built-in annotation; a metadata key cannot take its name",
                        name.name
                    ),
                );
            }
            let mut targets = BTreeSet::new();
            for words in &metadata.targets {
                let target = words
                    .iter()
                    .map(|w| w.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let span = words.first().map_or(name.span, |w| w.span);
                if !TARGETS.contains(&target.as_str()) {
                    self.error(
                        file,
                        span,
                        format!(
                            "`{target}` is not a metadata target; targets are {}",
                            TARGETS.join(", ")
                        ),
                    );
                } else if !targets.insert(target.clone()) {
                    self.error(file, span, format!("`{target}` is listed twice"));
                }
            }
            for conflict in &metadata.conflicts {
                let known = MODIFIERS.contains(&conflict.name.as_str())
                    || BUILT_IN.contains(&conflict.name.as_str())
                    || matches!(
                        self.decl(&conflict.name).map(|d| &d.item.decl),
                        Some(Decl::Metadata(_))
                    );
                if conflict.name == name.name {
                    self.error(
                        file,
                        conflict.span,
                        "a metadata key cannot conflict with itself",
                    );
                } else if !known {
                    self.error(
                        file,
                        conflict.span,
                        format!(
                            "`{}` is not a metadata key, built-in annotation or modifier",
                            conflict.name
                        ),
                    );
                }
            }
            if let Some(retention) = &metadata.retention
                && !matches!(retention.name.as_str(), "source" | "runtime")
            {
                self.error(file, retention.span, "retention is `source` or `runtime`");
            }
            match &metadata.ty {
                Some(MetaType::Enum(values)) => {
                    let mut seen = BTreeSet::new();
                    for value in values {
                        if !seen.insert(value.name.name.as_str()) {
                            self.error(
                                file,
                                value.name.span,
                                format!("`{}` is listed twice", value.name.name),
                            );
                        }
                    }
                }
                Some(MetaType::Type(ty)) => {
                    let Some(id) = self.resolve_type(file, &ty.name) else {
                        continue;
                    };
                    if !self.metadata_type(&id, true) {
                        self.error(
                            file,
                            ty.name.span,
                            "a metadata value is a scalar, an enum or a structure of those (decision 44)",
                        );
                    }
                    self.constraint_block(file, &ty.constraints);
                    let on = match prelude(&ty.name.name) {
                        Some((_, kind)) => on_kind(kind),
                        None => On::Other,
                    };
                    if prelude(&ty.name.name).is_some() {
                        self.check_constraints(file, &ty.constraints, on, true);
                    }
                    self.named_types.insert(id.clone());
                    types.push(id);
                }
                None => {}
            }
        }
        // Lower the value types for checking values against, apart from the
        // service's shapes: their problems are reported where they are
        // declared, and a type used only by metadata adds no requirement.
        let requirements = self.requirements.clone();
        let reported = self.diagnostics.len();
        let unplaced = std::mem::take(&mut self.unplaced);
        for id in types {
            self.visit(&id);
        }
        self.diagnostics.truncate(reported);
        self.unplaced = unplaced;
        self.requirements = requirements;
        self.meta_shapes = std::mem::take(&mut self.shapes);
    }

    /// Decision 44: a scalar, an enum, or (at the top) a structure of those.
    fn metadata_type(&self, id: &ShapeId, top: bool) -> bool {
        if id.is_prelude() {
            return prelude_kind(id.name()).is_some_and(|kind| kind != SimpleKind::Document);
        }
        match self.decl(id.name()).map(|d| &d.item.decl) {
            Some(Decl::Scalar(s)) => s.kind.name != "document",
            Some(Decl::Enum(_)) => true,
            Some(Decl::Structure(s)) if top => s.members.iter().all(|member| match &member.kind {
                MemberKind::Field { ty: Some(ty), .. } => {
                    let id = match prelude(&ty.name.name) {
                        Some((name, _)) => smithy(name),
                        None => self.ref_id(&ty.name.name),
                    };
                    self.metadata_type(&id, false)
                }
                _ => false,
            }),
            _ => false,
        }
    }

    /// A metadata value against its declared type and constraints.
    fn check_metadata_value(
        &mut self,
        file: usize,
        span: Span,
        name: &str,
        ty: &TypeRef,
        value: &Json,
    ) {
        let id = match prelude(&ty.name.name) {
            Some((prelude, _)) => smithy(prelude),
            None => self.ref_id(&ty.name.name),
        };
        if !self.meta_shapes.contains_key(&id) {
            // The declaration's own problem, already reported.
            return;
        }
        // Constraint problems were reported with the declaration.
        let reported = self.diagnostics.len();
        let own = self.constraint_block(file, &ty.constraints);
        self.diagnostics.truncate(reported);
        let mut constraints = match self.meta_shapes.get(&id) {
            Some(Shape::Simple(s)) => s.constraints.clone(),
            _ => Constraints::default(),
        };
        if own.constraints.length.is_some() {
            constraints.length = own.constraints.length;
        }
        if own.constraints.range.is_some() {
            constraints.range = own.constraints.range;
        }
        if own.constraints.pattern.is_some() {
            constraints.pattern = own.constraints.pattern;
        }
        let what = format!("the value of `@{name}`");
        for problem in
            crucible_smithy::validate::value(&self.meta_shapes, &what, &id, &constraints, value)
        {
            self.error(file, span, problem);
        }
    }

    fn no_args(&mut self, file: usize, annotation: &Annotation) {
        if !annotation.args.is_empty() {
            let name = &annotation.name.name;
            self.error(
                file,
                annotation.span,
                format!("`@{name}` takes no arguments"),
            );
        }
    }

    fn metadata_value(
        &mut self,
        file: usize,
        annotation: &Annotation,
        metadata: &Metadata,
    ) -> Json {
        let name = &annotation.name.name;
        match (&metadata.ty, annotation.args.as_slice()) {
            (None, []) => Json::Object(JsonMap::new()),
            (None, _) => {
                self.error(file, annotation.span, format!("`@{name}` takes no value"));
                Json::Object(JsonMap::new())
            }
            (Some(_), []) => {
                self.error(file, annotation.span, format!("`@{name}` needs a value"));
                Json::Null
            }
            (Some(MetaType::Enum(values)), [arg]) if arg.key.is_none() => {
                let given = match &arg.value {
                    Value::Ident(ident) => Some(ident.name.clone()),
                    Value::Str(s) => Some(s.value.clone()),
                    _ => None,
                };
                match given {
                    Some(given) if values.iter().any(|v| v.name.name == given) => {
                        Json::String(given)
                    }
                    _ => {
                        let allowed: Vec<&str> =
                            values.iter().map(|v| v.name.name.as_str()).collect();
                        self.error(
                            file,
                            arg.value.span(),
                            format!("`@{name}` is one of {}", allowed.join(", ")),
                        );
                        Json::Null
                    }
                }
            }
            (Some(MetaType::Type(ty)), [arg]) if arg.key.is_none() => {
                let json = self.json(file, &arg.value);
                self.check_metadata_value(file, arg.value.span(), name, ty, &json);
                json
            }
            (Some(ty), args) => {
                let mut map = JsonMap::new();
                for arg in args {
                    match &arg.key {
                        Some(Key::Name(key)) => {
                            let value = self.json(file, &arg.value);
                            if map.insert(key.name.clone(), value).is_some() {
                                self.error(
                                    file,
                                    key.span,
                                    format!("`{}` is given twice", key.name),
                                );
                            }
                        }
                        _ => self.error(
                            file,
                            arg.value.span(),
                            format!("`@{name}` takes one value or `key: value` pairs"),
                        ),
                    }
                }
                let map = Json::Object(map);
                if let MetaType::Type(ty) = ty {
                    self.check_metadata_value(file, annotation.span, name, ty, &map);
                }
                map
            }
        }
    }

    fn constraint_block(&mut self, file: usize, block: &[Constraint]) -> ConstraintInfo {
        let mut info = ConstraintInfo::default();
        for constraint in block {
            let name = constraint.name.name.as_str();
            match (name, &constraint.value) {
                ("length", ConstraintValue::Range { min, max }) => {
                    let mut bounds = Bounds {
                        min: None,
                        max: None,
                    };
                    let mut value = JsonMap::new();
                    for (key, literal, slot) in
                        [("min", min, &mut bounds.min), ("max", max, &mut bounds.max)]
                    {
                        let Some(literal) = literal else {
                            continue;
                        };
                        match literal.text.parse::<u64>() {
                            Ok(number) => {
                                *slot = Some(number);
                                value.insert(key.into(), Json::Number(number.into()));
                            }
                            Err(_) => self.error(
                                file,
                                literal.span,
                                "a length is a whole number of zero or more",
                            ),
                        }
                    }
                    if let (Some(min), Some(max)) = (bounds.min, bounds.max)
                        && min > max
                    {
                        self.error(file, constraint.span, "the minimum length exceeds the maximum");
                    }
                    info.constraints.length = Some(bounds);
                    info.traits.insert(smithy("length"), Json::Object(value));
                }
                ("range", ConstraintValue::Range { min, max }) => {
                    let mut value = JsonMap::new();
                    let min = min.as_ref().and_then(|n| self.number(file, n));
                    let max = max.as_ref().and_then(|n| self.number(file, n));
                    if let Some(min) = &min {
                        value.insert("min".into(), Json::Number(min.clone()));
                    }
                    if let Some(max) = &max {
                        value.insert("max".into(), Json::Number(max.clone()));
                    }
                    if let (Some(lo), Some(hi)) = (
                        min.as_ref().and_then(Number::as_f64),
                        max.as_ref().and_then(Number::as_f64),
                    ) && lo > hi
                    {
                        self.error(file, constraint.span, "the minimum exceeds the maximum");
                    }
                    info.constraints.range = Some(Bounds { min, max });
                    info.traits.insert(smithy("range"), Json::Object(value));
                }
                ("pattern", ConstraintValue::Value(Value::Str(pattern))) => {
                    info.constraints.pattern = Some(pattern.value.clone());
                    info.traits
                        .insert(smithy("pattern"), Json::String(pattern.value.clone()));
                }
                ("unique", ConstraintValue::Flag) => {
                    info.constraints.unique_items = true;
                    info.traits
                        .insert(smithy("uniqueItems"), Json::Object(JsonMap::new()));
                }
                ("format", ConstraintValue::Value(Value::Ident(format))) => {
                    let parsed = match format.name.as_str() {
                        "date-time" => Some(TimestampFormat::DateTime),
                        "http-date" => Some(TimestampFormat::HttpDate),
                        "epoch-seconds" => Some(TimestampFormat::EpochSeconds),
                        _ => None,
                    };
                    match parsed {
                        Some(parsed) => {
                            info.format = Some(parsed);
                            info.traits.insert(
                                smithy("timestampFormat"),
                                Json::String(format.name.clone()),
                            );
                        }
                        None => self.error(
                            file,
                            format.span,
                            "a timestamp format is `date-time`, `http-date` or `epoch-seconds`",
                        ),
                    }
                }
                ("mediaType", ConstraintValue::Value(Value::Str(media))) => {
                    info.media_type = Some(media.value.clone());
                    info.traits
                        .insert(smithy("mediaType"), Json::String(media.value.clone()));
                }
                _ => self.error(
                    file,
                    constraint.span,
                    format!(
                        "unknown or malformed constraint `{name}`: constraints are `length a..b`, \
                         `range a..b`, `pattern \"..\"`, `unique`, `format ..` and `mediaType \"..\"`"
                    ),
                ),
            }
        }
        info
    }

    /// What kind of value a constraint would constrain at `id`.
    fn on_shape(&self, id: &ShapeId) -> On {
        match (self.shapes.get(id), self.pending.get(id)) {
            (Some(Shape::Simple(simple)), _) => on_kind(simple.kind),
            (Some(Shape::Enum(_)), _) => On::StringEnum,
            (Some(Shape::List(_)), _) | (None, Some(Pending::List(_))) => On::List,
            (Some(Shape::Map(_)), _) | (None, Some(Pending::Map(_))) => On::Map,
            _ => On::Other,
        }
    }

    /// Each constraint applies to some kinds of value, once, and a pattern
    /// must be one the generated validators can run. `member` positions
    /// report `mediaType` themselves.
    fn check_constraints(&mut self, file: usize, block: &[Constraint], on: On, member: bool) {
        let mut seen = BTreeSet::new();
        for constraint in block {
            let name = constraint.name.name.as_str();
            if !seen.insert(name) {
                self.error(
                    file,
                    constraint.span,
                    format!("`{name}` is given twice; give each constraint once"),
                );
                continue;
            }
            let (applies, what) = match name {
                "length" => (
                    matches!(on, On::Text | On::Blob | On::List | On::Map),
                    "a string, blob, list or map",
                ),
                "range" => (on == On::Number, "a number"),
                "pattern" => (on == On::Text, "a string"),
                "unique" => (on == On::List, "a list"),
                "format" => (on == On::Timestamp, "a timestamp"),
                "mediaType" if !member => (matches!(on, On::Text | On::Blob), "a blob or string"),
                _ => continue,
            };
            if !applies {
                self.error(
                    file,
                    constraint.span,
                    format!("`{name}` constrains {what}, which this is not"),
                );
            }
            if name == "pattern"
                && let ConstraintValue::Value(Value::Str(pattern)) = &constraint.value
                && let Some(problem) = crucible_smithy::validate::pattern_problem(&pattern.value)
            {
                self.error(
                    file,
                    pattern.span,
                    format!("this pattern cannot be checked: {problem}"),
                );
            }
        }
    }

    /// A member's default, as the IR carries it: checked against the target
    /// and its constraints, and an enum value named by its symbol written as
    /// its wire value.
    fn default_value(
        &mut self,
        file: usize,
        span: Span,
        value: &Value,
        target: &ShapeId,
        constraints: &Constraints,
    ) -> Json {
        let mut json = self.json(file, value);
        // A recursive target is still pending: its kind is all there is.
        let pending = match self.pending.get(target) {
            Some(Pending::Union) => Some(UNION_DEFAULT),
            Some(Pending::Structure) => Some(STRUCTURE_DEFAULT),
            _ => None,
        };
        if let Some(message) = pending {
            self.error(file, span, message);
            return json;
        }
        match self.shapes.get(target) {
            Some(Shape::Simple(simple)) if simple.kind == SimpleKind::Timestamp => {
                self.error(
                    file,
                    span,
                    "timestamp defaults are not supported by the compiler yet",
                );
                return json;
            }
            Some(Shape::Union(_)) => {
                self.error(file, span, UNION_DEFAULT);
                return json;
            }
            Some(Shape::Structure(_)) => {
                self.error(file, span, STRUCTURE_DEFAULT);
                return json;
            }
            _ => {}
        }
        for problem in resolve_enum_symbols(&self.shapes, target, value, &mut json) {
            self.error(file, span, problem);
        }
        for problem in crucible_smithy::validate::value(
            &self.shapes,
            "the default",
            target,
            constraints,
            &json,
        ) {
            self.error(file, span, problem);
        }
        json
    }

    /// Bindings carry a member in a part of an HTTP message, so each one is
    /// valid only where that part exists, and a member has at most one.
    #[allow(clippy::too_many_arguments)]
    fn check_member_bindings(
        &mut self,
        file: usize,
        member: &Member,
        name: &str,
        name_span: Span,
        required: bool,
        target: &ShapeId,
        annotations: &Annotations,
        context: MemberContext<'_>,
    ) {
        let at = |binding: &str| {
            member
                .annotations
                .iter()
                .find(|a| a.name.name == binding)
                .map_or(name_span, |a| a.span)
        };
        let bindings: Vec<&str> = [
            ("query", annotations.query.is_some()),
            ("header", annotations.header.is_some()),
            ("payload", annotations.payload),
        ]
        .into_iter()
        .filter_map(|(binding, set)| set.then_some(binding))
        .collect();
        if let [first, second, ..] = bindings.as_slice() {
            self.error(
                file,
                at(second),
                format!("`{name}` is bound once: `@{first}` and `@{second}` cannot both apply"),
            );
        }
        let ignored = if context.included {
            format!(
                "`{}` includes it, where it would be ignored",
                context.owner.name()
            )
        } else {
            "here it would be ignored".to_owned()
        };
        for binding in &bindings {
            let allowed = match (context.site, *binding) {
                (Site::Input, _) => true,
                (Site::Output | Site::Error, "query") => false,
                (Site::Output | Site::Error, _) => true,
                (Site::Nested | Site::Structure, _) => false,
            };
            if !allowed {
                let place = if *binding == "query" {
                    "an operation input"
                } else {
                    "an operation input, output or error"
                };
                self.misplaced(
                    file,
                    at(binding),
                    format!("`@{binding}` binds a member of {place}; {ignored}"),
                    context,
                );
            }
        }
        if annotations.token {
            if context.site != Site::Input {
                self.misplaced(
                    file,
                    at("token"),
                    format!("`@token` marks an operation input member; {ignored}"),
                    context,
                );
            }
            if required {
                self.error(
                    file,
                    name_span,
                    format!("the idempotency token `{name}` is optional (`{name}?`): a client fills it when absent"),
                );
            }
            if self.on_shape(target) != On::Text {
                self.error(file, name_span, "an idempotency token is a string");
            }
        }
    }

    /// A binding where its part of the message does not exist. In a declared
    /// structure it waits until the package is lowered: it is an error only if
    /// something uses the structure as a type (`Site::Structure`).
    fn misplaced(&mut self, file: usize, span: Span, message: String, context: MemberContext<'_>) {
        let problem = FileDiagnostic {
            file: Some(file),
            diagnostic: Diagnostic::new(span, message),
        };
        if context.site == Site::Structure {
            self.unplaced
                .entry(context.owner.clone())
                .or_default()
                .push(problem);
        } else {
            self.diagnostics.push(problem);
        }
    }

    /// Reports the binding problems of the declared structures something
    /// uses as a type: a member's, a collection's, a metadata value's or a
    /// resource entry's. A structure nothing uses that way is a bundle of
    /// members for `include`, and its bindings were judged where it was
    /// included (decision 40).
    fn report_unplaced(&mut self) {
        let mut used = self.named_types.clone();
        for shape in self.shapes.values().chain(self.meta_shapes.values()) {
            match shape {
                Shape::Structure(s) | Shape::Union(s) => {
                    used.extend(s.members.iter().map(|m| m.target.clone()));
                }
                Shape::List(list) => {
                    used.insert(list.member.target.clone());
                }
                Shape::Map(map) => {
                    used.insert(map.key.target.clone());
                    used.insert(map.value.target.clone());
                }
                _ => {}
            }
        }
        for (owner, problems) in std::mem::take(&mut self.unplaced) {
            if used.contains(&owner) {
                self.diagnostics.extend(problems);
            }
        }
    }

    /// Modifiers on a declaration: each position accepts its own.
    fn modifiers(&mut self, file: usize, item: &Item, allowed: &[&str]) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for modifier in &item.modifiers {
            if allowed.contains(&modifier.name.as_str()) {
                out.insert(modifier.name.clone());
            } else {
                let keyword = decl_keyword(&item.decl);
                self.error(
                    file,
                    modifier.span,
                    format!("`{}` does not apply to a {keyword}", modifier.name),
                );
            }
        }
        out
    }

    // ---- shapes -------------------------------------------------------------

    fn visit(&mut self, id: &ShapeId) {
        if self.shapes.contains_key(id) {
            return;
        }
        // A shape reached again while it is lowered: recursion, which the
        // IR records by target id; `pending` answers what its referrers ask.
        if self.visiting.contains(id) {
            return;
        }
        self.visiting.insert(id.clone());
        if let Some(decl) = self.decl(id.name()) {
            match decl.item.decl {
                Decl::Structure(_) => {
                    self.pending.insert(id.clone(), Pending::Structure);
                }
                Decl::Union(_) => {
                    self.pending.insert(id.clone(), Pending::Union);
                }
                _ => {}
            }
        }
        let shape = if id.namespace() == "smithy.api" {
            self.prelude_shape(id)
        } else if let Some(body) = self.bodies.get(id).cloned() {
            self.locate(id.clone(), body.file, body.body.span);
            Some(self.body_shape(id, &body))
        } else if let Some(decl) = self.decl(id.name()) {
            self.locate(id.clone(), decl.file, decl.item.span);
            self.item_shape(id, decl)
        } else {
            None
        };
        self.visiting.remove(id);
        self.pending.remove(id);
        if let Some(shape) = shape {
            self.shapes.insert(id.clone(), shape);
        }
    }

    fn prelude_shape(&mut self, id: &ShapeId) -> Option<Shape> {
        if id.name() == "Unit" {
            return Some(Shape::Unit { internal: false });
        }
        let kind = prelude_kind(id.name())?;
        self.kind_requirements(id, kind);
        Some(Shape::Simple(Simple {
            internal: false,
            sensitive: false,
            kind,
            constraints: Constraints::default(),
            timestamp_format: None,
            docs: None,
            notes: DocNotes::default(),
            traits: Traits::new(),
        }))
    }

    fn kind_requirements(&mut self, id: &ShapeId, kind: SimpleKind) {
        match kind {
            SimpleKind::BigInteger => self.require(id, Feature::BigInteger, smithy("BigInteger")),
            SimpleKind::BigDecimal => self.require(id, Feature::BigDecimal, smithy("BigDecimal")),
            _ => {}
        }
    }

    /// Shape-level traits shared by every declaration: annotations,
    /// documentation and the `internal`/`sensitive` modifiers.
    fn shape_traits(
        &mut self,
        file: usize,
        item: &Item,
        keyword: &'static str,
        modifiers: &BTreeSet<String>,
    ) -> (Traits, DocNotes, Option<String>) {
        let annotations = self.annotations(
            file,
            &item.annotations,
            &item.modifiers,
            Position::Shape(keyword),
        );
        let mut traits = annotations.traits;
        let docs = self.doc(file, item.doc.as_ref());
        if let Some(docs) = &docs {
            traits.insert(smithy("documentation"), Json::String(docs.clone()));
        }
        for modifier in ["internal", "sensitive"] {
            if modifiers.contains(modifier) {
                traits.insert(smithy(modifier), Json::Object(JsonMap::new()));
            }
        }
        (traits, annotations.notes, docs)
    }

    fn item_shape(&mut self, id: &ShapeId, decl: DeclRef<'a>) -> Option<Shape> {
        let file = decl.file;
        let item = decl.item;
        Some(match &item.decl {
            Decl::Scalar(scalar) => {
                let modifiers = self.modifiers(file, item, &["internal", "sensitive"]);
                let (mut traits, notes, docs) = self.shape_traits(file, item, "scalar", &modifiers);
                let Some((_, kind)) = prelude(&scalar.kind.name) else {
                    self.error(file, scalar.kind.span, "unknown scalar kind");
                    return None;
                };
                self.kind_requirements(id, kind);
                let info = self.constraint_block(file, &scalar.constraints);
                self.check_constraints(file, &scalar.constraints, on_kind(kind), false);
                traits.extend(info.traits);
                Shape::Simple(Simple {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    kind,
                    constraints: info.constraints,
                    timestamp_format: info.format,
                    docs,
                    notes,
                    traits,
                })
            }
            Decl::Enum(enumeration) => {
                let modifiers = self.modifiers(file, item, &["internal", "sensitive"]);
                let (traits, notes, docs) = self.shape_traits(file, item, "enum", &modifiers);
                let integer = match &enumeration.base {
                    None => false,
                    Some(base) if base.name == "integer" => true,
                    Some(base) => {
                        self.error(file, base.span, "an enum is a string enum or `: integer`");
                        false
                    }
                };
                let mut names = BTreeSet::new();
                for value in &enumeration.values {
                    if !names.insert(value.name.name.as_str()) {
                        self.error(
                            file,
                            value.name.span,
                            format!("enum value `{}` is declared twice", value.name.name),
                        );
                    }
                }
                if integer {
                    self.require(id, Feature::IntEnum, smithy("IntEnum"));
                    let mut seen = BTreeSet::new();
                    let mut values = Vec::new();
                    for value in &enumeration.values {
                        if let Some(lowered) = self.int_enum_value(file, id, value) {
                            if !seen.insert(lowered.value) {
                                self.error(
                                    file,
                                    value.span,
                                    format!("enum value {} appears twice", lowered.value),
                                );
                            }
                            values.push(lowered);
                        }
                    }
                    Shape::IntEnum(IntEnum {
                        internal: modifiers.contains("internal"),
                        sensitive: modifiers.contains("sensitive"),
                        values,
                        docs,
                        notes,
                        traits,
                    })
                } else {
                    let mut seen = BTreeSet::new();
                    let mut values = Vec::new();
                    for value in &enumeration.values {
                        let lowered = self.enum_value(file, id, value);
                        if !seen.insert(lowered.value.clone()) {
                            self.error(
                                file,
                                value.span,
                                format!("enum value {:?} appears twice", lowered.value),
                            );
                        }
                        values.push(lowered);
                    }
                    Shape::Enum(Enum {
                        internal: modifiers.contains("internal"),
                        sensitive: modifiers.contains("sensitive"),
                        values,
                        docs,
                        notes,
                        traits,
                    })
                }
            }
            Decl::List(list) => {
                let modifiers = self.modifiers(file, item, &["internal", "sensitive", "sparse"]);
                let (mut traits, notes, docs) = self.shape_traits(file, item, "list", &modifiers);
                // Its own constraints first: a member reaching this list
                // again (recursion) reads them while it is pending.
                let info = self.constraint_block(file, &list.constraints);
                self.check_constraints(file, &list.constraints, On::List, false);
                self.pending
                    .insert(id.clone(), Pending::List(info.constraints.clone()));
                let target = self.resolve_type(file, &list.member)?;
                let member = self.collection_member(id, "member", &target);
                traits.extend(info.traits);
                if modifiers.contains("sparse") {
                    self.require(id, Feature::Sparse, smithy("sparse"));
                }
                Shape::List(List {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    sparse: modifiers.contains("sparse"),
                    member,
                    constraints: info.constraints,
                    docs,
                    notes,
                    traits,
                })
            }
            Decl::Map(map) => {
                let modifiers = self.modifiers(file, item, &["internal", "sensitive", "sparse"]);
                let (mut traits, notes, docs) = self.shape_traits(file, item, "map", &modifiers);
                let info = self.constraint_block(file, &map.constraints);
                self.check_constraints(file, &map.constraints, On::Map, false);
                self.pending
                    .insert(id.clone(), Pending::Map(info.constraints.clone()));
                let key = self.resolve_type(file, &map.key)?;
                let value = self.resolve_type(file, &map.value)?;
                self.visit(&key);
                if !matches!(self.on_shape(&key), On::Text | On::StringEnum) {
                    self.error(
                        file,
                        map.key.span,
                        "a map key is a string or a string enum: JSON object keys are text",
                    );
                }
                let key = self.collection_member(id, "key", &key);
                let value = self.collection_member(id, "value", &value);
                traits.extend(info.traits);
                if modifiers.contains("sparse") {
                    self.require(id, Feature::Sparse, smithy("sparse"));
                }
                Shape::Map(Map {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    sparse: modifiers.contains("sparse"),
                    key,
                    value,
                    constraints: info.constraints,
                    docs,
                    notes,
                    traits,
                })
            }
            Decl::Structure(structure) => {
                let modifiers = self.modifiers(file, item, &["internal", "sensitive", "strict"]);
                let (mut traits, notes, docs) =
                    self.shape_traits(file, item, "structure", &modifiers);
                if modifiers.contains("strict") {
                    traits.insert(
                        ShapeId::new("crucible.protocols", "strict"),
                        Json::Object(JsonMap::new()),
                    );
                }
                let resource = structure.resource.as_ref().and_then(|resource| {
                    match self.decl(&resource.name).map(|d| &d.item.decl) {
                        Some(Decl::Resource(_)) => Some(resource.name.as_str()),
                        _ => {
                            self.error(
                                file,
                                resource.span,
                                format!("`{}` is not a resource", resource.name),
                            );
                            None
                        }
                    }
                });
                let others: Vec<&str> = resource
                    .map_or_else(Vec::new, |first| self.view_siblings(file, structure, first));
                let context = MemberContext {
                    owner: id,
                    resource,
                    also: &others,
                    union: false,
                    labels: &[],
                    site: Site::Structure,
                    included: false,
                    collection: false,
                    role: None,
                    view: resource.is_some(),
                };
                let members = self.members(file, &structure.members, context);
                Shape::Structure(IrStructure {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    strict: modifiers.contains("strict"),
                    input: false,
                    members,
                    error: None,
                    docs,
                    notes,
                    traits,
                })
            }
            Decl::Union(union) => {
                let modifiers = self.modifiers(file, item, &["internal", "sensitive"]);
                let (traits, notes, docs) = self.shape_traits(file, item, "union", &modifiers);
                let context = MemberContext {
                    owner: id,
                    resource: None,
                    also: &[],
                    union: true,
                    labels: &[],
                    site: Site::Nested,
                    included: false,
                    collection: false,
                    role: None,
                    view: false,
                };
                let members = self.members(file, &union.members, context);
                Shape::Union(IrStructure {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    strict: false,
                    input: false,
                    members,
                    error: None,
                    docs,
                    notes,
                    traits,
                })
            }
            Decl::Error(error) => {
                let modifiers = self.modifiers(file, item, &["internal", "sensitive"]);
                let (mut traits, notes, docs) = self.shape_traits(file, item, "error", &modifiers);
                let mut status = None;
                let mut retryable = None;
                let mut members = Vec::new();
                for entry in &error.entries {
                    match entry {
                        ErrorEntry::Status(literal) if status.is_some() => {
                            self.error(file, literal.span, "an error has one `status`");
                        }
                        ErrorEntry::Retryable { span, .. } if retryable.is_some() => {
                            self.error(file, *span, "an error is `retryable` once");
                        }
                        ErrorEntry::Status(literal) => match literal.text.parse::<u16>() {
                            Ok(code) if (400..=599).contains(&code) => status = Some(code),
                            _ => self.error(
                                file,
                                literal.span,
                                "an error status is a 4xx or 5xx HTTP status code",
                            ),
                        },
                        ErrorEntry::Retryable { throttling, .. } => {
                            retryable = Some(Retryable {
                                throttling: *throttling,
                            });
                        }
                        ErrorEntry::Member(member) => members.push(member.clone()),
                    }
                }
                let Some(status) = status else {
                    self.error(
                        file,
                        error.name.span,
                        "an error needs a `status`, such as `status 404`",
                    );
                    return None;
                };
                let fault = if status >= 500 {
                    Fault::Server
                } else {
                    Fault::Client
                };
                traits.insert(
                    smithy("error"),
                    Json::String(
                        if fault == Fault::Server {
                            "server"
                        } else {
                            "client"
                        }
                        .into(),
                    ),
                );
                traits.insert(smithy("httpError"), Json::Number(status.into()));
                if let Some(retryable) = &retryable {
                    let mut value = JsonMap::new();
                    if retryable.throttling {
                        value.insert("throttling".into(), Json::Bool(true));
                    }
                    traits.insert(smithy("retryable"), Json::Object(value));
                }
                let context = MemberContext {
                    owner: id,
                    resource: None,
                    also: &[],
                    union: false,
                    labels: &[],
                    site: Site::Error,
                    included: false,
                    collection: false,
                    role: None,
                    view: false,
                };
                let members = self.members(file, &members, context);
                let info = self.error_info(id, &members, fault, status, retryable);
                Shape::Structure(IrStructure {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    strict: false,
                    input: false,
                    members,
                    error: Some(info),
                    docs,
                    notes,
                    traits,
                })
            }
            _ => return None,
        })
    }

    /// `internal` is the one modifier an enum value takes (decision 19):
    /// sensitivity belongs to the whole enum, since a value's name is public.
    fn enum_value_internal(&mut self, file: usize, value: &EnumValue) -> bool {
        let mut internal = false;
        for modifier in &value.modifiers {
            if modifier.name == "internal" {
                internal = true;
            } else {
                self.error(
                    file,
                    modifier.span,
                    format!(
                        "`{}` does not apply to an enum value; only `internal` does",
                        modifier.name
                    ),
                );
            }
        }
        internal
    }

    fn enum_value(&mut self, file: usize, id: &ShapeId, value: &EnumValue) -> IrEnumValue {
        let internal = self.enum_value_internal(file, value);
        let annotations = self.annotations(
            file,
            &value.annotations,
            &value.modifiers,
            Position::EnumValue,
        );
        let wire = match &value.value {
            None => value.name.name.clone(),
            Some(Value::Str(s)) => s.value.clone(),
            Some(other) => {
                self.error(file, other.span(), "a string enum value is a string");
                value.name.name.clone()
            }
        };
        self.locate(id.with_member(&value.name.name), file, value.span);
        IrEnumValue {
            internal,
            name: value.name.name.clone(),
            value: wire,
            docs: self.doc(file, value.doc.as_ref()),
            notes: annotations.notes,
            traits: annotations.traits,
        }
    }

    fn int_enum_value(
        &mut self,
        file: usize,
        id: &ShapeId,
        value: &EnumValue,
    ) -> Option<IntEnumValue> {
        let internal = self.enum_value_internal(file, value);
        let annotations = self.annotations(
            file,
            &value.annotations,
            &value.modifiers,
            Position::EnumValue,
        );
        let number = match &value.value {
            Some(Value::Number(n)) => n.text.parse::<i32>().ok(),
            _ => None,
        };
        let Some(number) = number else {
            self.error(
                file,
                value.span,
                "an integer enum value needs `= <32-bit integer>`",
            );
            return None;
        };
        self.locate(id.with_member(&value.name.name), file, value.span);
        Some(IntEnumValue {
            internal,
            name: value.name.name.clone(),
            value: number,
            docs: self.doc(file, value.doc.as_ref()),
            notes: annotations.notes,
            traits: annotations.traits,
        })
    }

    fn body_shape(&mut self, id: &ShapeId, source: &BodySource<'a>) -> Shape {
        let file = source.file;
        let mut traits = Traits::new();
        let marker = match source.body.kind {
            BodyKind::Input => "input",
            BodyKind::Output => "output",
        };
        traits.insert(smithy(marker), Json::Object(JsonMap::new()));
        let mut strict = false;
        for modifier in &source.body.modifiers {
            if modifier.name == "strict" && source.body.kind == BodyKind::Input {
                strict = true;
                traits.insert(
                    ShapeId::new("crucible.protocols", "strict"),
                    Json::Object(JsonMap::new()),
                );
            } else {
                self.error(
                    file,
                    modifier.span,
                    format!(
                        "`{}` does not apply here; only an input can be `strict`",
                        modifier.name
                    ),
                );
            }
        }
        let attachment = source.operation.attachment.as_ref();
        let resource = attachment.map(|a| a.resource.name.as_str());
        let input = source.body.kind == BodyKind::Input;
        let labels: &[String] = if input { &source.labels } else { &[] };
        let context = MemberContext {
            owner: id,
            resource,
            also: &[],
            union: false,
            labels,
            site: if input { Site::Input } else { Site::Output },
            included: false,
            collection: input
                && attachment.is_some_and(|a| {
                    a.collection || matches!(a.role.name.as_str(), "creates" | "lists")
                }),
            role: attachment.map(|a| a.role.name.as_str()),
            view: false,
        };
        let members = self.members(file, &source.body.members, context);
        let docs = self.doc(file, source.body.doc.as_ref());
        if let Some(docs) = &docs {
            traits.insert(smithy("documentation"), Json::String(docs.clone()));
        }
        Shape::Structure(IrStructure {
            internal: false,
            sensitive: false,
            strict,
            input: source.body.kind == BodyKind::Input,
            members,
            error: None,
            docs,
            notes: DocNotes::default(),
            traits,
        })
    }

    fn collection_member(&mut self, owner: &ShapeId, name: &str, target: &ShapeId) -> IrMember {
        self.visit(target);
        let constraints = self.constraints_of(target);
        let explicit = self.explicit_format(target);
        IrMember {
            internal: false,
            sensitive: false,
            name: name.to_owned(),
            target: target.clone(),
            required: false,
            default: None,
            wire_default: None,
            client_optional: false,
            idempotency_token: false,
            mutability: None,
            constraints,
            timestamp_format: None,
            wire_timestamp_format: explicit.unwrap_or_else(|| {
                self.protocol.member_timestamp_format(
                    &Traits::new(),
                    self.http_inputs.contains(owner),
                    self.http_outputs.contains(owner),
                )
            }),
            docs: None,
            notes: DocNotes::default(),
            traits: Traits::new(),
        }
    }

    fn constraints_of(&self, id: &ShapeId) -> Constraints {
        match (self.shapes.get(id), self.pending.get(id)) {
            (Some(Shape::Simple(s)), _) => s.constraints.clone(),
            (Some(Shape::List(l)), _) => l.constraints.clone(),
            (Some(Shape::Map(m)), _) => m.constraints.clone(),
            (None, Some(Pending::List(c) | Pending::Map(c))) => c.clone(),
            _ => Constraints::default(),
        }
    }

    fn explicit_format(&self, id: &ShapeId) -> Option<TimestampFormat> {
        match self.shapes.get(id) {
            Some(Shape::Simple(s)) => s.timestamp_format,
            _ => None,
        }
    }

    // ---- members -----------------------------------------------------------

    fn members(
        &mut self,
        file: usize,
        members: &[Member],
        context: MemberContext<'_>,
    ) -> Vec<IrMember> {
        let mut out = Vec::new();
        let mut including = Vec::new();
        let mut names = BTreeSet::new();
        self.members_into(
            file,
            members,
            context,
            None,
            &mut out,
            &mut names,
            &mut including,
        );
        let mut tokens = out.iter().filter(|member| member.idempotency_token);
        if let (Some(first), Some(second)) = (tokens.next(), tokens.next()) {
            let message = format!(
                "`{}` and `{}` are both idempotency tokens; an input has at most one",
                first.name, second.name
            );
            self.error_at(&context.owner.with_member(&second.name), &message);
        }
        out
    }

    /// `at` is where an included member is reported: the `include` that
    /// brought it in, in the file being lowered.
    #[allow(clippy::too_many_arguments)]
    fn members_into(
        &mut self,
        file: usize,
        members: &[Member],
        context: MemberContext<'_>,
        at: Option<(usize, Span)>,
        out: &mut Vec<IrMember>,
        names: &mut BTreeSet<String>,
        including: &mut Vec<String>,
    ) {
        // Decision 34d: the entries `$*` included, by name, at their index in
        // `out`; and the `$name`s written before it, which it would repeat.
        let mut star: Option<BTreeMap<String, usize>> = None;
        let mut before_star: BTreeMap<String, Span> = BTreeMap::new();
        for member in members {
            if let MemberKind::All(span) = &member.kind {
                self.expand_all(
                    file,
                    member,
                    *span,
                    context,
                    out,
                    names,
                    &mut star,
                    &before_star,
                );
                continue;
            }
            if let MemberKind::Elided {
                name,
                alias,
                optional,
            } = &member.kind
                && context.view
            {
                match &star {
                    Some(included) => {
                        if let Some(&index) = included.get(&name.name) {
                            let bare = alias.is_none()
                                && member.annotations.is_empty()
                                && member.modifiers.is_empty()
                                && member.doc.is_none();
                            if *optional && bare {
                                // Loosened for this view, where `$*` put it.
                                out[index].required = false;
                                out[index].traits.remove(&smithy("required"));
                            } else {
                                self.error(
                                    file,
                                    member.span,
                                    format!(
                                        "`${0}` is already included by `$*`; after it, only `${0}?` names it again, to make it optional in this view",
                                        name.name
                                    ),
                                );
                            }
                            continue;
                        }
                    }
                    None => {
                        before_star.entry(name.name.clone()).or_insert(member.span);
                    }
                }
            }
            if let MemberKind::Include(name) = &member.kind {
                if !member.annotations.is_empty() {
                    self.error(file, member.span, "`include` takes no annotations");
                }
                let Some(decl) = self.decl(&name.name) else {
                    self.error(
                        file,
                        name.span,
                        format!("unknown structure `{}`", name.name),
                    );
                    continue;
                };
                let Decl::Structure(structure) = &decl.item.decl else {
                    self.error(
                        file,
                        name.span,
                        format!(
                            "`include` copies a structure's members; `{}` is not a structure",
                            name.name
                        ),
                    );
                    continue;
                };
                if including.contains(&name.name) {
                    self.error(file, name.span, format!("`{}` includes itself", name.name));
                    continue;
                }
                including.push(name.name.clone());
                let at = at.or(Some((file, name.span)));
                // An included `for Resource` structure elides against its own
                // resource, wherever it is included; the including operation's
                // collection role does not carry over to that resource.
                let own = structure.resource.as_ref().map(|r| r.name.as_str());
                // The included view's other resources stay checked; so do the
                // including view's, when both are views of the same resource.
                let mut others: Vec<&str> = Vec::new();
                let inherited = if own.is_some() && own == context.resource {
                    context.also
                } else {
                    &[]
                };
                for other in structure
                    .also
                    .iter()
                    .map(|r| r.name.as_str())
                    .chain(inherited.iter().copied())
                {
                    if Some(other) != own && !others.contains(&other) {
                        others.push(other);
                    }
                }
                // Its members are a representation unless they land in an
                // input, where they bind what the operation takes; the
                // operation's role applies only to its own resource.
                let context = MemberContext {
                    resource: own.or(context.resource),
                    also: if own.is_some() { &others } else { context.also },
                    included: true,
                    collection: context.collection && own.is_none(),
                    role: context
                        .role
                        .filter(|_| own.is_none() || own == context.resource),
                    view: context.view || (own.is_some() && context.site != Site::Input),
                    ..context
                };
                self.members_into(
                    decl.file,
                    &structure.members,
                    context,
                    at,
                    out,
                    names,
                    including,
                );
                including.pop();
                continue;
            }
            if let Some(lowered) = self.member(file, member, context) {
                if !names.insert(lowered.name.clone()) {
                    let (file, span) = at.unwrap_or((file, member.span));
                    self.error(
                        file,
                        span,
                        format!(
                            "member `{}` is declared twice in `{}`",
                            lowered.name,
                            context.owner.name()
                        ),
                    );
                    continue;
                }
                out.push(lowered);
            }
        }
    }

    /// `$*` (decision 34d): the view's own identifiers under their
    /// representation names, then every property that is not `writeonly`, in
    /// declaration order, each with the property's presence. Inherited
    /// identifiers are not included; a view that shows one names it.
    #[allow(clippy::too_many_arguments)]
    fn expand_all(
        &mut self,
        file: usize,
        member: &Member,
        span: Span,
        context: MemberContext<'_>,
        out: &mut Vec<IrMember>,
        names: &mut BTreeSet<String>,
        star: &mut Option<BTreeMap<String, usize>>,
        before_star: &BTreeMap<String, Span>,
    ) {
        if !context.view {
            let message = if context.site == Site::Input {
                "`$*` belongs in a `for` view: an input binds what the operation takes, not the resource's state"
            } else {
                "`$*` belongs in a `for` view, where it includes the resource's identifiers and readable properties"
            };
            self.error(file, span, message);
            return;
        }
        if star.is_some() {
            self.error(file, span, "`$*` appears once in a view");
            return;
        }
        if member.doc.is_some() || !member.annotations.is_empty() || !member.modifiers.is_empty() {
            self.error(
                file,
                member.span,
                "`$*` takes no documentation, annotations or modifiers: each member it includes keeps its entry's",
            );
        }
        let mut included = BTreeMap::new();
        let Some(resource) = context.resource else {
            *star = Some(included);
            return;
        };
        let Some((_, own)) = self.chain(resource).into_iter().next() else {
            *star = Some(included);
            return;
        };
        let entries = own
            .entries
            .iter()
            .filter(|e| e.kind == ResourceEntryKind::Id)
            .chain(own.entries.iter().filter(|e| {
                e.kind == ResourceEntryKind::Property
                    && !e.modifiers.iter().any(|m| m.name == "writeonly")
            }));
        for entry in entries {
            let name = &entry.name.name;
            if name.is_empty() {
                continue;
            }
            // Of several resources, `$*` shows what they all have; a member
            // they declare differently is reported by the member itself.
            if !context.also.iter().all(|other| {
                self.resource_entry(other, name)
                    .is_some_and(|found| found.own && found.entry.kind == entry.kind)
            }) {
                continue;
            }
            if let Some(&earlier) = before_star.get(name) {
                self.error(
                    file,
                    earlier,
                    format!(
                        "`${name}` is written before `$*`, which includes it: remove it, or loosen it with `${name}?` after `$*`"
                    ),
                );
                continue;
            }
            let synthetic = Member {
                doc: None,
                annotations: Vec::new(),
                modifiers: Vec::new(),
                kind: MemberKind::Elided {
                    name: Ident {
                        name: name.clone(),
                        span,
                    },
                    alias: None,
                    optional: false,
                },
                span,
            };
            let Some(lowered) = self.member(file, &synthetic, context) else {
                continue;
            };
            if !names.insert(lowered.name.clone()) {
                self.error(
                    file,
                    span,
                    format!(
                        "`$*` includes `{}`, which `{}` already declares",
                        lowered.name,
                        context.owner.name()
                    ),
                );
                continue;
            }
            included.insert(name.clone(), out.len());
            out.push(lowered);
        }
        *star = Some(included);
    }

    /// The `for resource` view a member type is, or the items or values of,
    /// if any.
    fn nested_view(&self, name: &str, resource: &str, depth: usize) -> Option<String> {
        // Lists of lists end; a cycle of collections is reported elsewhere.
        if depth > 8 {
            return None;
        }
        match self.decl(name).map(|d| &d.item.decl) {
            Some(Decl::Structure(structure)) if view_of(structure, resource) => {
                Some(name.to_owned())
            }
            Some(Decl::List(list)) => self.nested_view(&list.member.name, resource, depth + 1),
            Some(Decl::Map(map)) => self.nested_view(&map.value.name, resource, depth + 1),
            _ => None,
        }
    }

    /// `resource` and its ancestors, nearest first, with their files; a
    /// cycle (reported by `check_resources`) ends the chain.
    fn chain(&self, resource: &str) -> Vec<(usize, &'a Resource)> {
        let mut out = Vec::new();
        let mut current = Some(resource.to_owned());
        let mut seen = BTreeSet::new();
        while let Some(name) = current {
            if !seen.insert(name.clone()) {
                break;
            }
            let Some(decl) = self.decl(&name) else {
                break;
            };
            let Decl::Resource(r) = &decl.item.decl else {
                break;
            };
            out.push((decl.file, r));
            current = r.parent.as_ref().map(|parent| parent.name.clone());
        }
        out
    }

    /// What `$name` names in `resource`: one of its own identifiers or
    /// properties, or an identifier it inherits. An ancestor's properties
    /// are not inherited (decision 31): a child is addressed by its
    /// parents' identifiers, not described by their state.
    fn resource_entry(&self, resource: &str, name: &str) -> Option<EntryRef<'a>> {
        self.chain(resource)
            .into_iter()
            .enumerate()
            .find_map(|(depth, (file, r))| {
                r.entries
                    .iter()
                    .find(|entry| {
                        entry.name.name == name
                            && (depth == 0 || entry.kind == ResourceEntryKind::Id)
                    })
                    .map(|entry| EntryRef {
                        file,
                        entry,
                        own: depth == 0,
                    })
            })
    }

    /// The ancestor of `resource` that declares a property `name`, which
    /// `resource` does not inherit: for the diagnostic of a `$name` that
    /// expects it to.
    fn ancestor_property(&self, resource: &str, name: &str) -> Option<String> {
        self.chain(resource).into_iter().skip(1).find_map(|(_, r)| {
            r.entries
                .iter()
                .any(|e| e.kind == ResourceEntryKind::Property && e.name.name == name)
                .then(|| r.name.name.clone())
        })
    }

    /// Decision 34a for a member that binds a resource entry: the checks of
    /// a write against the property's keyword, and the mutability the member
    /// carries. Views, outputs and the inputs of `creates`, `puts` and
    /// `updates` carry the property's value, and so its mutability; a filter
    /// (`@query`, or another role's input) carries none, since the
    /// infrastructure generators hide every shape holding a write-only
    /// member. An identifier is not a property and carries none.
    fn bound_mutability(
        &mut self,
        file: usize,
        member: &Member,
        name: &str,
        entry: &ResourceEntry,
        annotations: &Annotations,
        context: MemberContext<'_>,
    ) -> Option<Mutability> {
        let Some(declared) = entry_mutability(entry) else {
            return annotations.mutability;
        };
        let property = &entry.name.name;
        let representation = context.view || context.site == Site::Output;
        let role = context
            .role
            .filter(|role| context.site == Site::Input && WRITES.contains(role));
        let write = role.filter(|_| annotations.query.is_none());
        // A custom collection action creates (34g): what it binds carries
        // the property's value, though its input is not checked.
        let creates_unchecked = context.site == Site::Input
            && context.role == Some("on")
            && context.collection
            && annotations.query.is_none();
        if representation && declared.write_only() {
            self.error(
                file,
                member.span,
                format!(
                    "`{property}` is {}: a view or an output never returns it",
                    mutability_keyword(declared)
                ),
            );
        }
        match (write, declared) {
            (
                Some("updates"),
                Mutability::Read | Mutability::CreateAndRead | Mutability::Create,
            ) => {
                self.error(
                    file,
                    member.span,
                    format!(
                        "an `updates` input cannot bind `{property}`, which is {}: {}",
                        mutability_keyword(declared),
                        if declared == Mutability::Read {
                            "the service assigns it"
                        } else {
                            "it is set by `creates` or `puts` and fixed afterwards"
                        }
                    ),
                );
            }
            (Some(role @ ("creates" | "puts")), Mutability::Read) => {
                self.error(
                    file,
                    member.span,
                    format!(
                        "a `{role}` input cannot bind `{property}`, which is `readonly`: the service assigns it"
                    ),
                );
            }
            _ => {}
        }
        if let Some(explicit) = annotations.mutability
            && explicit != declared
        {
            let at = member
                .annotations
                .iter()
                .find(|a| a.name.name == "mutability")
                .map_or(member.span, |a| a.span);
            self.error(
                file,
                at,
                format!(
                    "`@mutability({})` disagrees with `{property}`, which is {} (`{}`): `{name}` takes its mutability from the property",
                    mutability_annotation(explicit),
                    mutability_keyword(declared),
                    mutability_annotation(declared)
                ),
            );
        }
        if representation || write.is_some() || creates_unchecked {
            Some(declared)
        } else {
            annotations.mutability
        }
    }

    /// The other resources of a `for A, B` view, checked: each is a resource,
    /// named once and not the first. Names that fail are reported and left out.
    fn view_siblings<'s>(
        &mut self,
        file: usize,
        structure: &'s crate::ast::Structure,
        first: &str,
    ) -> Vec<&'s str> {
        let mut out: Vec<&str> = Vec::new();
        for resource in &structure.also {
            let name = resource.name.as_str();
            if !matches!(
                self.decl(name).map(|d| &d.item.decl),
                Some(Decl::Resource(_))
            ) {
                self.error(file, resource.span, format!("`{name}` is not a resource"));
            } else if name == first || out.contains(&name) {
                self.error(
                    file,
                    resource.span,
                    format!("`{name}` is named twice in the view's `for`"),
                );
            } else {
                out.push(name);
            }
        }
        out
    }

    /// A member of a view, lowered against its resource; in a `for A, B` view
    /// also against each other resource, which must give the same member.
    fn member(
        &mut self,
        file: usize,
        member: &Member,
        context: MemberContext<'_>,
    ) -> Option<IrMember> {
        let lowered = self.member_of(file, member, context)?;
        if context.also.is_empty() || !matches!(member.kind, MemberKind::Elided { .. }) {
            return Some(lowered);
        }
        let first = context.resource.unwrap_or_default();
        for other in context.also {
            let before = self.diagnostics.len();
            let sibling = MemberContext {
                resource: Some(other),
                also: &[],
                ..context
            };
            let twin = self.member_of(file, member, sibling);
            // What was wrong with the member itself was said for the first
            // resource; say it once.
            let known: Vec<_> = self.diagnostics[..before].to_vec();
            let added: Vec<_> = self.diagnostics.split_off(before);
            self.diagnostics
                .extend(added.into_iter().filter(|new| !known.contains(new)));
            let Some(twin) = twin else { continue };
            let differing = member_differences(&lowered, &twin);
            if differing.is_empty() {
                continue;
            }
            let message = format!(
                "`{}` is not the same member of `{first}` and `{other}` ({}): a view of several resources shows only what each declares identically",
                lowered.name,
                differing.join(", ")
            );
            // A view lowered for itself and again where it is included says
            // the same thing at the same place.
            let said = self.diagnostics.iter().any(|item| {
                item.file == Some(file)
                    && item.diagnostic.span == member.span
                    && item.diagnostic.message == message
            });
            if !said {
                self.error(file, member.span, message);
            }
        }
        Some(lowered)
    }

    fn member_of(
        &mut self,
        file: usize,
        member: &Member,
        context: MemberContext<'_>,
    ) -> Option<IrMember> {
        let mut bound: Option<EntryRef<'a>> = None;
        let (name, name_span, ty, ty_file, required, default, doc) = match &member.kind {
            MemberKind::Include(_) | MemberKind::All(_) => return None,
            MemberKind::Elided {
                name,
                alias,
                optional,
            } => {
                let Some(resource) = context.resource else {
                    self.error(
                        file,
                        name.span,
                        format!("`${}` names a resource member, but there is no resource here: use it in an operation attached to a resource or a structure declared `for Resource`", name.name),
                    );
                    return None;
                };
                let Some(found) = self.resource_entry(resource, &name.name) else {
                    let message = match self.ancestor_property(resource, &name.name) {
                        Some(ancestor) => format!(
                            "resource `{resource}` has no identifier or property `{0}`: `{0}` is a property of its ancestor `{ancestor}`, and a child inherits only its ancestors' identifiers",
                            name.name
                        ),
                        None => format!(
                            "resource `{resource}` has no identifier or property `{}`",
                            name.name
                        ),
                    };
                    self.error(file, name.span, message);
                    return None;
                };
                let entry = found.entry;
                bound = Some(found);
                // Views and outputs are the resource's representations: an own
                // identifier takes its spelling there (34f), and a property its
                // declared presence (34e). An input binds by entry name, and
                // asks for what the operation needs.
                let representation = context.view || context.site == Site::Output;
                let spelled = entry
                    .alias
                    .as_ref()
                    .filter(|_| representation && found.own && entry.kind == ResourceEntryKind::Id);
                let (member_name, member_span) = match (alias, spelled) {
                    (Some(alias), _) => (alias.name.clone(), alias.span),
                    (None, Some(spelled)) => (spelled.name.clone(), name.span),
                    (None, None) => (name.name.clone(), name.span),
                };
                let doc = member.doc.as_ref().or(entry.doc.as_ref());
                (
                    member_name,
                    member_span,
                    Some(&entry.ty),
                    found.file,
                    !optional && !(representation && entry.optional),
                    None,
                    doc,
                )
            }
            MemberKind::Field {
                name,
                optional,
                ty,
                default,
            } => {
                if *optional && default.is_some() {
                    self.error(
                        file,
                        name.span,
                        "a member is optional (`?`) or has a default (`= value`), not both",
                    );
                }
                if ty.is_none() && !context.union {
                    self.error(
                        file,
                        name.span,
                        format!("member `{}` needs a type", name.name),
                    );
                    return None;
                }
                (
                    name.name.clone(),
                    name.span,
                    ty.as_ref(),
                    file,
                    !optional && default.is_none(),
                    default.as_ref(),
                    member.doc.as_ref(),
                )
            }
        };
        let required = required && !context.union;
        let mut annotations = self.annotations(
            file,
            &member.annotations,
            &member.modifiers,
            Position::Member,
        );
        let mutability = match bound {
            Some(found) => {
                self.bound_mutability(file, member, &name, found.entry, &annotations, context)
            }
            None => annotations.mutability,
        };
        let target = match ty {
            Some(ty) => self.resolve_type(ty_file, &ty.name)?,
            None => smithy("Unit"),
        };
        self.visit(&target);
        let own = match ty {
            Some(TypeRef { constraints, .. }) => self.constraint_block(ty_file, constraints),
            None => ConstraintInfo::default(),
        };
        let on = self.on_shape(&target);
        self.check_constraints(ty_file, ty.map_or(&[][..], |ty| &ty.constraints), on, true);
        self.check_member_bindings(
            file,
            member,
            &name,
            name_span,
            required,
            &target,
            &annotations,
            context,
        );
        let mut traits = std::mem::take(&mut annotations.traits);
        traits.extend(own.traits);
        if name.contains('-') {
            self.error(
                file,
                name_span,
                format!("a member name is an identifier: `{name}` cannot contain `-`"),
            );
        }
        // Decision 33: a member binds a resource entry only by elision. The
        // hint is only for a member that `$name` could replace: one of the
        // entry's type, and not an identifier a collection operation may not
        // bind (decision 32), where `$name` would be an error.
        if matches!(member.kind, MemberKind::Field { .. })
            && let Some(resource) = context.resource
            && let Some(found) = self.resource_entry(resource, &name)
            && self.type_id(&found.entry.ty.name.name).as_ref() == Some(&target)
            && !(context.collection && self.identifiers(resource).1.contains(&name))
        {
            self.warning(
                file,
                name_span,
                format!(
                    "`{name}` is a plain member and does not bind `{resource}`'s `{name}`; did you mean `${name}`?"
                ),
            );
        }
        // Decision 34a checks a write property by property: a view nested in
        // a writing input binds nothing, and its keywords go unchecked.
        if context.site == Site::Input
            && context
                .role
                .is_some_and(|role| WRITES.contains(&role) || role == "on")
            && let Some(resource) = context.resource
            && let MemberKind::Field { ty: Some(ty), .. } = &member.kind
            && let Some(view) = self.nested_view(&ty.name.name, resource, 0)
        {
            self.warning(
                file,
                name_span,
                format!(
                    "`{name}` is typed by `{view}`, a view of `{resource}`: an input binds the properties it writes with `$name`, so their keywords are checked; a nested view's are not, and it counts as no binding"
                ),
            );
        }
        let mut internal = false;
        let mut sensitive = false;
        for modifier in &member.modifiers {
            match modifier.name.as_str() {
                "internal" => internal = true,
                "sensitive" => sensitive = true,
                other => self.error(
                    file,
                    modifier.span,
                    format!("`{other}` does not apply to a member; `internal` and `sensitive` do"),
                ),
            }
        }
        if internal {
            traits.insert(smithy("internal"), Json::Object(JsonMap::new()));
        }
        if sensitive {
            traits.insert(smithy("sensitive"), Json::Object(JsonMap::new()));
        }
        if let Some(query) = &annotations.query {
            let parameter = query.clone().unwrap_or_else(|| name.clone());
            traits.insert(smithy("httpQuery"), Json::String(parameter));
        }
        if let Some(header) = &annotations.header {
            traits.insert(smithy("httpHeader"), Json::String(header.clone()));
        }
        if annotations.payload {
            traits.insert(smithy("httpPayload"), Json::Object(JsonMap::new()));
        }
        if annotations.token {
            traits.insert(smithy("idempotencyToken"), Json::Object(JsonMap::new()));
        }
        if context.labels.contains(&name) {
            let explicit =
                annotations.query.is_some() || annotations.header.is_some() || annotations.payload;
            if explicit {
                self.error(
                    file,
                    name_span,
                    format!("`{name}` binds the URI label {{{name}}} by name; it cannot also be a query parameter, header or payload"),
                );
            }
            traits.insert(smithy("httpLabel"), Json::Object(JsonMap::new()));
        }
        if required {
            traits.insert(smithy("required"), Json::Object(JsonMap::new()));
        }
        // A doc borrowed from the resource entry is checked with the entry.
        self.doc(file, member.doc.as_ref());
        let docs = doc_text(doc);
        if let Some(docs) = &docs {
            traits.insert(smithy("documentation"), Json::String(docs.clone()));
        }
        // A member's constraint replaces its type's, as in Smithy. One that
        // loosens it accepts what the type refuses, which is usually a
        // mistake (a dropped bound), so it is a warning, not an error: the
        // weather model loosens a name on purpose to exercise output checks.
        let mut constraints = self.constraints_of(&target);
        if let (Some(own_length), Some(length)) = (&own.constraints.length, &constraints.length)
            && !within(
                own_length.min,
                own_length.max,
                length.min,
                length.max,
                |a, b| a.cmp(b),
            )
        {
            self.warning(
                file,
                name_span,
                format!("`{name}`'s `length` loosens its type's, which it replaces; give both bounds to keep the type's"),
            );
        }
        if let (Some(own_range), Some(range)) = (&own.constraints.range, &constraints.range)
            && !within(
                own_range.min.as_ref().and_then(Number::as_f64),
                own_range.max.as_ref().and_then(Number::as_f64),
                range.min.as_ref().and_then(Number::as_f64),
                range.max.as_ref().and_then(Number::as_f64),
                |a, b| a.total_cmp(b),
            )
        {
            self.warning(
                file,
                name_span,
                format!("`{name}`'s `range` loosens its type's, which it replaces; give both bounds to keep the type's"),
            );
        }
        if own.constraints.length.is_some() {
            constraints.length = own.constraints.length;
        }
        if own.constraints.range.is_some() {
            constraints.range = own.constraints.range;
        }
        if own.constraints.pattern.is_some() {
            constraints.pattern = own.constraints.pattern;
        }
        constraints.unique_items |= own.constraints.unique_items;
        let default =
            default.map(|value| self.default_value(file, name_span, value, &target, &constraints));
        if let Some(default) = &default {
            traits.insert(smithy("default"), default.clone());
        }
        if own.media_type.is_some() {
            self.error(
                file,
                name_span,
                "`mediaType` belongs on a blob or string shape, not a member",
            );
        }
        let wire_timestamp_format = own
            .format
            .or_else(|| self.explicit_format(&target))
            .unwrap_or_else(|| {
                self.protocol.member_timestamp_format(
                    &traits,
                    self.http_inputs.contains(context.owner),
                    self.http_outputs.contains(context.owner),
                )
            });
        self.locate(context.owner.with_member(&name), file, member.span);
        Some(IrMember {
            internal,
            sensitive,
            name: name.clone(),
            target,
            required,
            wire_default: default.clone(),
            default,
            // Decision 20: a required member is required in generated clients too.
            client_optional: false,
            idempotency_token: annotations.token,
            mutability,
            constraints,
            timestamp_format: own.format,
            wire_timestamp_format,
            docs,
            notes: annotations.notes,
            traits,
        })
    }

    // ---- bindings ----------------------------------------------------------

    fn bindings(&mut self, owner: &ShapeId, members: &[IrMember], request: bool) -> Bindings {
        let mut bindings = Bindings::default();
        for member in members {
            let traits = &member.traits;
            let string = |name: &str| {
                traits
                    .get(&smithy(name))
                    .and_then(Json::as_str)
                    .map(str::to_owned)
            };
            if traits.contains_key(&smithy("httpPayload")) {
                if bindings.payload.is_some() {
                    self.error_at(
                        &owner.with_member(&member.name),
                        "only one member may be the payload",
                    );
                } else {
                    bindings.payload = self.payload(owner, member);
                }
            } else if request && traits.contains_key(&smithy("httpLabel")) {
                bindings.labels.push(member.name.clone());
            } else if let Some(parameter) = string("httpQuery").filter(|_| request) {
                bindings.query.push(Binding {
                    member: member.name.clone(),
                    name: parameter,
                });
            } else if let Some(header) = string("httpHeader") {
                // Generated clients send their release in this header.
                if header.eq_ignore_ascii_case("crucible-version") {
                    self.error_at(
                        &owner.with_member(&member.name),
                        "`Crucible-Version` carries the API release a client was generated for; bind the member to another header",
                    );
                }
                bindings.headers.push(Binding {
                    member: member.name.clone(),
                    name: header,
                });
            } else {
                bindings.body.push(member.name.clone());
            }
        }
        // Duplicate names, binding targets and a payload beside body members
        // are checked by crucible_smithy::validate.
        self.header_encodings(owner, members, &mut bindings);
        self.binding_timestamp_formats(members, &mut bindings);
        bindings
    }

    fn payload(&mut self, owner: &ShapeId, member: &IrMember) -> Option<Payload> {
        let (kind, traits) = match self.shapes.get(&member.target) {
            Some(Shape::Simple(s)) => match s.kind {
                SimpleKind::Blob => (PayloadKind::Blob, &s.traits),
                SimpleKind::String => (PayloadKind::String, &s.traits),
                SimpleKind::Document => (PayloadKind::Document, &s.traits),
                _ => {
                    self.error_at(
                        &owner.with_member(&member.name),
                        "a payload is a blob, string, structure, union or document",
                    );
                    return None;
                }
            },
            Some(Shape::Enum(e)) => (PayloadKind::String, &e.traits),
            Some(Shape::Structure(s) | Shape::Union(s)) => (PayloadKind::Document, &s.traits),
            _ => {
                self.error_at(
                    &owner.with_member(&member.name),
                    "a payload is a blob, string, structure, union or document",
                );
                return None;
            }
        };
        let media_type = traits
            .get(&smithy("mediaType"))
            .and_then(Json::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                match kind {
                    PayloadKind::Blob => "application/octet-stream",
                    PayloadKind::String => "text/plain",
                    PayloadKind::Document => "application/json",
                }
                .to_owned()
            });
        if kind != PayloadKind::Blob {
            self.require(
                &owner.with_member(&member.name),
                Feature::NonBlobPayload,
                smithy("httpPayload"),
            );
        }
        Some(Payload {
            kind,
            member: member.name.clone(),
            media_type,
        })
    }

    fn header_encodings(&mut self, owner: &ShapeId, members: &[IrMember], bindings: &mut Bindings) {
        for binding in &bindings.headers {
            let Some(member) = members.iter().find(|m| m.name == binding.member) else {
                continue;
            };
            let mut target = self.shapes.get(&member.target);
            if let Some(Shape::List(list)) = target {
                target = self.shapes.get(&list.member.target);
            }
            let media = match target {
                Some(Shape::Simple(s)) if s.kind == SimpleKind::String => {
                    s.traits.contains_key(&smithy("mediaType"))
                }
                Some(Shape::Enum(e)) => e.traits.contains_key(&smithy("mediaType")),
                _ => false,
            };
            if media {
                self.require(
                    &owner.with_member(&binding.member),
                    Feature::MediaTypeHeaders,
                    smithy("mediaType"),
                );
                bindings
                    .header_encodings
                    .insert(binding.member.clone(), HeaderEncoding::Base64);
            }
        }
    }

    fn binding_timestamp_formats(&self, members: &[IrMember], bindings: &mut Bindings) {
        let bound = bindings
            .headers
            .iter()
            .map(|b| (b.member.clone(), true))
            .chain(bindings.query.iter().map(|b| (b.member.clone(), false)))
            .chain(bindings.labels.iter().map(|name| (name.clone(), false)))
            .collect::<Vec<_>>();
        for (name, header) in bound {
            let Some(member) = members.iter().find(|m| m.name == name) else {
                continue;
            };
            let mut own = member.timestamp_format;
            let mut target = self.shapes.get(&member.target);
            if let Some(Shape::List(list)) = target {
                own = list.member.timestamp_format;
                target = self.shapes.get(&list.member.target);
            }
            let Some(Shape::Simple(simple)) = target else {
                continue;
            };
            if simple.kind != SimpleKind::Timestamp {
                continue;
            }
            let explicit = own.or(simple.timestamp_format);
            let document = explicit.unwrap_or_else(|| self.protocol.timestamp_format(false));
            let binding = explicit.unwrap_or_else(|| {
                if header {
                    self.protocol.timestamp_format(true)
                } else {
                    TimestampFormat::DateTime
                }
            });
            if binding != document {
                bindings.timestamp_formats.insert(name, binding);
            }
        }
    }

    fn error_info(
        &mut self,
        id: &ShapeId,
        members: &[IrMember],
        fault: Fault,
        status: u16,
        retryable: Option<Retryable>,
    ) -> ErrorInfo {
        for member in members {
            if member.traits.contains_key(&smithy("httpPayload")) {
                self.require(
                    &id.with_member(&member.name),
                    Feature::ErrorPayload,
                    smithy("httpPayload"),
                );
            }
        }
        let bindings = self.bindings(id, members, false);
        ErrorInfo {
            protocol_headers: self.protocol.response_headers(&bindings, true),
            body_discriminator: Some(id.name().to_owned()),
            discriminator_header: self.protocol.error_header().map(str::to_owned),
            timestamp_formats: bindings.timestamp_formats,
            wire_code: id.name().to_owned(),
            payload: bindings.payload,
            header_encodings: bindings.header_encodings,
            fault,
            status,
            retryable,
            headers: bindings.headers,
        }
    }

    /// `paginated(inputToken: a, outputToken: b, items: c, pageSize: d)`:
    /// each key known and given once.
    fn pagination_args(&mut self, file: usize, args: &[crate::ast::Arg]) -> JsonMap<String, Json> {
        let mut out = JsonMap::new();
        for arg in args {
            match (&arg.key, &arg.value) {
                (Some(Key::Name(key)), Value::Ident(member)) => {
                    if !matches!(
                        key.name.as_str(),
                        "inputToken" | "outputToken" | "items" | "pageSize"
                    ) {
                        self.error(
                            file,
                            key.span,
                            format!(
                                "`{}` is not a pagination key; the keys are inputToken, outputToken, items and pageSize",
                                key.name
                            ),
                        );
                    } else if out
                        .insert(key.name.clone(), Json::String(member.name.clone()))
                        .is_some()
                    {
                        self.error(file, key.span, format!("`{}` is given twice", key.name));
                    }
                }
                _ => self.error(
                    file,
                    arg.value.span(),
                    "`paginated` takes `inputToken:`, `outputToken:`, `items:` and `pageSize:` member names",
                ),
            }
        }
        out
    }

    // ---- operations --------------------------------------------------------
    fn uri(&mut self, file: usize, span: Span, pattern: &str) -> Uri {
        let (path, query) = pattern.split_once('?').unwrap_or((pattern, ""));
        if !path.starts_with('/') {
            self.error(file, span, "a URI starts with `/`");
        }
        if path.contains("//") {
            self.error(file, span, "a URI has no empty path segments");
        }
        if query.contains(['{', '}']) {
            self.error(
                file,
                span,
                "the query string of a URI cannot contain labels",
            );
        }
        let mut segments = Vec::new();
        let mut greedy_labels = Vec::new();
        for part in path.split('/').filter(|part| !part.is_empty()) {
            let label = part.strip_prefix('{').and_then(|p| p.strip_suffix('}'));
            if let Some(label) = label {
                let name = label.strip_suffix('+').unwrap_or(label);
                if name.is_empty() {
                    self.error(
                        file,
                        span,
                        "`{}` is an empty URI label: name the member it binds",
                    );
                    continue;
                }
                if name.contains(['{', '}']) {
                    self.error(
                        file,
                        span,
                        format!("URI segment `{part}` holds more than one label; a segment is one label or literal text"),
                    );
                    continue;
                }
                if !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                    || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    self.error(
                        file,
                        span,
                        format!(
                            "URI label `{{{name}}}` must name a member, which is an identifier"
                        ),
                    );
                    continue;
                }
            }
            match label {
                Some(label) if label.ends_with('+') => {
                    let label = label.trim_end_matches('+').to_owned();
                    greedy_labels.push(label.clone());
                    segments.push(Segment::Label(label));
                }
                Some(label) => segments.push(Segment::Label(label.to_owned())),
                None if part.contains(['{', '}']) => self.error(
                    file,
                    span,
                    format!("URI segment `{part}` mixes a label with literal text"),
                ),
                None => segments.push(Segment::Literal(part.to_owned())),
            }
        }
        let mut seen = BTreeSet::new();
        for segment in &segments {
            if let Segment::Label(name) = segment
                && !seen.insert(name.clone())
            {
                self.error(file, span, format!("URI label {{{name}}} appears twice"));
            }
        }
        if greedy_labels.len() > 1 {
            self.error(file, span, "a URI has at most one greedy label");
        }
        let query = query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| match pair.split_once('=') {
                Some((key, value)) => (key.to_owned(), Some(value.to_owned())),
                None => (pair.to_owned(), None),
            })
            .collect();
        Uri {
            segments,
            query,
            greedy_labels,
        }
    }

    fn operation(
        &mut self,
        decl: DeclRef<'a>,
        service_errors: &[ShapeId],
        default_pagination: &JsonMap<String, Json>,
        default_auth: &[ShapeId],
    ) -> Option<IrOperation> {
        let file = decl.file;
        let item = decl.item;
        let Decl::Operation(operation) = &item.decl else {
            return None;
        };
        let id = self.ref_id(&operation.name.name);
        self.locate(id.clone(), file, item.span);
        let modifiers = self.modifiers(
            file,
            item,
            &["readonly", "idempotent", "public", "internal"],
        );
        let annotations = self.annotations(
            file,
            &item.annotations,
            &item.modifiers,
            Position::Operation,
        );
        let mut traits = annotations.traits;

        let mut http = None;
        let mut input = None;
        let mut output = None;
        let mut own_errors = Vec::new();
        let mut pagination = None;
        let mut examples = Vec::new();
        for entry in &operation.entries {
            match entry {
                OpEntry::Http(h) => {
                    if http.replace(h).is_some() {
                        self.error(file, h.span, "an operation has one `http` clause");
                    }
                }
                OpEntry::Body(body) => {
                    let slot = match body.kind {
                        BodyKind::Input => &mut input,
                        BodyKind::Output => &mut output,
                    };
                    if slot.replace(body).is_some() {
                        self.error(file, body.span, "an operation has one input and one output");
                    }
                }
                OpEntry::Errors(errors, _) => {
                    for error in errors {
                        if own_errors.iter().any(|e: &&Ident| e.name == error.name) {
                            self.error(
                                file,
                                error.span,
                                format!("`{}` is listed twice", error.name),
                            );
                        } else {
                            own_errors.push(error);
                        }
                    }
                }
                OpEntry::Call(call) if call.name.name == "paginated" => {
                    if pagination.replace(call).is_some() {
                        self.error(file, call.span, "an operation has one `paginated` clause");
                    }
                }
                OpEntry::Call(call) => self.error(
                    file,
                    call.name.span,
                    format!("unknown clause `{}`", call.name.name),
                ),
                OpEntry::Example(example) => examples.push(example),
                // Lowered with the service's permissions, below.
                OpEntry::Requirement(_) => {}
            }
        }

        let Some(http) = http else {
            self.error(
                file,
                operation.name.span,
                "an operation served over HTTP needs an `http METHOD \"/uri\"` clause",
            );
            // Still check what the operation declares, so one run lists
            // every problem.
            if input.is_some() {
                self.visit(&self.id(&self.naming.input(&operation.name.name)));
            }
            if output.is_some() {
                self.visit(&self.id(&self.naming.output(&operation.name.name)));
            }
            for error in own_errors {
                match self.decl(&error.name).map(|d| &d.item.decl) {
                    Some(Decl::Error(_)) => self.visit(&self.ref_id(&error.name)),
                    _ => self.error(
                        file,
                        error.span,
                        format!("`{}` is not an error", error.name),
                    ),
                }
            }
            return None;
        };
        let method = http.method.name.clone();
        if !matches!(
            method.as_str(),
            "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS"
        ) {
            self.error(
                file,
                http.method.span,
                format!("unknown HTTP method `{method}`"),
            );
        }
        let uri = self.uri(file, http.uri.span, &http.uri.value);
        if !uri.greedy_labels.is_empty() {
            self.require(&id, Feature::GreedyLabels, smithy("http"));
        }
        let status = match &http.status {
            None => 200,
            Some(literal) => match literal.text.parse::<u16>() {
                Ok(code) if (200..=299).contains(&code) => code,
                _ => {
                    self.error(
                        file,
                        literal.span,
                        "a success status is a 2xx HTTP status code",
                    );
                    200
                }
            },
        };
        let mut http_trait = JsonMap::new();
        http_trait.insert("method".into(), Json::String(method.clone()));
        http_trait.insert("uri".into(), Json::String(http.uri.value.clone()));
        if http.status.is_some() {
            http_trait.insert("code".into(), Json::Number(status.into()));
        }
        traits.insert(smithy("http"), Json::Object(http_trait));

        let (role, resource) = match &operation.attachment {
            Some(attachment) => {
                self.check_role(file, operation, attachment, input, &modifiers);
                match self.decl(&attachment.resource.name).map(|d| &d.item.decl) {
                    Some(Decl::Resource(_)) => {}
                    _ => self.error(
                        file,
                        attachment.resource.span,
                        format!("`{}` is not a resource", attachment.resource.name),
                    ),
                }
                (
                    Some(attachment.role.name.as_str()),
                    Some(self.ref_id(&attachment.resource.name)),
                )
            }
            None => (None, None),
        };

        let input_id = input.map(|_| self.id(&self.naming.input(&operation.name.name)));
        let output_id = output.map(|_| self.id(&self.naming.output(&operation.name.name)));
        let uri_labels: Vec<String> = uri.labels().map(str::to_owned).collect();
        if let Some(input_id) = &input_id {
            self.visit(input_id);
        }
        if let Some(output_id) = &output_id {
            self.visit(output_id);
        }
        let input_members = input_id
            .as_ref()
            .and_then(|id| match self.shapes.get(id) {
                Some(Shape::Structure(s)) => Some(s.members.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let output_members = output_id
            .as_ref()
            .and_then(|id| match self.shapes.get(id) {
                Some(Shape::Structure(s)) => Some(s.members.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let request = match &input_id {
            Some(input_id) => self.bindings(input_id, &input_members, true),
            None => Bindings::default(),
        };
        let response = match &output_id {
            Some(output_id) => self.bindings(output_id, &output_members, false),
            None => Bindings::default(),
        };
        // A member the source declares but lowering dropped, say for an
        // unknown type, already has its diagnostic; don't report it twice.
        let written: Vec<&str> = input
            .map(|body| {
                body.members
                    .iter()
                    .filter_map(|member| match &member.kind {
                        MemberKind::Field { name, .. } => Some(name.name.as_str()),
                        MemberKind::Elided { name, alias, .. } => {
                            Some(alias.as_ref().unwrap_or(name).name.as_str())
                        }
                        MemberKind::Include(_) | MemberKind::All(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        for label in &uri_labels {
            if !request.labels.contains(label) && !written.contains(&label.as_str()) {
                self.error(
                    file,
                    http.uri.span,
                    format!("URI label {{{label}}} matches no input member; add `{label}` (or `${label}`) to the input"),
                );
            }
        }
        // Decision 35c: these requests carry no body on the wire.
        if matches!(method.as_str(), "GET" | "HEAD" | "DELETE") {
            for member in request
                .body
                .iter()
                .chain(request.payload.iter().map(|p| &p.member))
            {
                self.error(
                    file,
                    http.method.span,
                    format!(
                        "a {method} request has no body: bind `{member}` with `@query` or `@header`"
                    ),
                );
            }
        }

        // Role and keywords together (decisions 26 and 32).
        let idempotency_token = input_members
            .iter()
            .find(|m| m.traits.contains_key(&smithy("idempotencyToken")))
            .map(|m| m.name.clone());
        let readonly = modifiers.contains("readonly") || matches!(role, Some("reads" | "lists"));
        // `updates` implies nothing about repeating: Smithy's update lifecycle
        // does not require idempotency, and an update that, say, appends a
        // version is not safe to retry. It says `idempotent` when it is.
        let idempotent = !readonly
            && (modifiers.contains("idempotent")
                || matches!(role, Some("puts" | "deletes"))
                || idempotency_token.is_some());
        if readonly && modifiers.contains("idempotent") {
            self.error(
                file,
                operation.name.span,
                "an operation is `readonly` or `idempotent`, not both",
            );
        }
        match method.as_str() {
            "GET" | "HEAD" if !readonly => self.error(
                file,
                http.method.span,
                format!("a {method} operation must be `readonly`"),
            ),
            "PUT" | "DELETE" if !readonly && !idempotent => self.error(
                file,
                http.method.span,
                format!("a {method} operation must be `idempotent`"),
            ),
            _ => {}
        }
        if readonly {
            traits.insert(smithy("readonly"), Json::Object(JsonMap::new()));
        }
        if idempotent {
            traits.insert(smithy("idempotent"), Json::Object(JsonMap::new()));
        }

        let mut protocol_headers = BTreeMap::new();
        if let Some(payload) = &request.payload {
            protocol_headers.insert("Content-Type".into(), payload.media_type.clone());
        } else if !request.body.is_empty() {
            protocol_headers.insert("Content-Type".into(), "application/json".into());
        }
        if request
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("Content-Type"))
        {
            protocol_headers.remove("Content-Type");
        }

        let mut errors = Vec::new();
        for error in own_errors {
            match self.decl(&error.name).map(|d| &d.item.decl) {
                Some(Decl::Error(_)) => errors.push(self.ref_id(&error.name)),
                _ => self.error(
                    file,
                    error.span,
                    format!("`{}` is not an error", error.name),
                ),
            }
        }
        let requirements = self.requirements(file, &id, operation);
        // Its clauses failed to lower, so the IR says `Undeclared` and the
        // shared check, which reads a public operation without
        // requirements as correct, cannot tell it declared some.
        if modifiers.contains("public") && self.unlowered_requirements.contains(&id) {
            self.error(
                file,
                operation.name.span,
                format!(
                    "permissions.public: `{}` is public, so there is no caller to authorize; remove its requirement declarations",
                    operation.name.name
                ),
            );
        }
        self.attach_not_found(&requirements, errors.len(), &mut errors);
        for error in service_errors {
            if !errors.contains(error) {
                errors.push(error.clone());
            }
        }
        for error in &errors {
            self.visit(error);
        }

        let paginated = pagination.and_then(|call| {
            let own = self.pagination_args(file, &call.args);
            let field = |name: &str| {
                own.get(name)
                    .or_else(|| default_pagination.get(name))
                    .and_then(Json::as_str)
                    .map(str::to_owned)
            };
            let paginated = Paginated {
                input_token: field("inputToken")?,
                output_token: field("outputToken")?,
                items: field("items"),
                page_size: field("pageSize"),
            };
            // Member paths and types are checked by crucible_smithy::validate.
            traits.insert(smithy("paginated"), Json::Object(own));
            Some(paginated)
        });
        if pagination.is_some() && paginated.is_none() {
            self.error(
                file,
                operation.name.span,
                "pagination needs `inputToken` and `outputToken`, on the operation or as the service default",
            );
        }

        let mut lowered_examples = Vec::new();
        let mut example_traits = Vec::new();
        for example in examples {
            let mut lowered = IrExample {
                title: example.title.value.clone(),
                docs: self.doc(file, example.doc.as_ref()),
                input: None,
                output: None,
                error: None,
                allow_constraint_errors: false,
            };
            let mut raw = JsonMap::new();
            raw.insert("title".into(), Json::String(lowered.title.clone()));
            if let Some(docs) = &lowered.docs {
                raw.insert("documentation".into(), Json::String(docs.clone()));
            }
            let mut parts = BTreeSet::new();
            for part in &example.parts {
                let (kind, span) = match part {
                    ExamplePart::Input(value) => ("input", value.span()),
                    ExamplePart::Output(value) => ("output", value.span()),
                    ExamplePart::Error(name, _) => ("error", name.span),
                };
                if !parts.insert(kind) {
                    self.error(file, span, format!("an example has one `{kind}`"));
                    continue;
                }
                match part {
                    ExamplePart::Input(value) => lowered.input = Some(self.json(file, value)),
                    ExamplePart::Output(value) => lowered.output = Some(self.json(file, value)),
                    ExamplePart::Error(name, value) => {
                        let shape = self.ref_id(&name.name);
                        let content = self.json(file, value);
                        if errors.contains(&shape) {
                            lowered.error = Some(ExampleError { shape, content });
                        } else {
                            // Reported here, at the name, rather than by the
                            // shared validator against the operation.
                            self.error(
                                file,
                                name.span,
                                format!("`{}` is not one of this operation's errors", name.name),
                            );
                        }
                    }
                }
            }
            if let Some(input) = &lowered.input {
                raw.insert("input".into(), input.clone());
            }
            if let Some(output) = &lowered.output {
                raw.insert("output".into(), output.clone());
            }
            if let Some(error) = &lowered.error {
                let mut value = JsonMap::new();
                value.insert("shapeId".into(), Json::String(error.shape.to_string()));
                value.insert("content".into(), error.content.clone());
                raw.insert("error".into(), Json::Object(value));
            }
            example_traits.push(Json::Object(raw));
            lowered_examples.push(lowered);
        }
        if !example_traits.is_empty() {
            traits.insert(smithy("examples"), Json::Array(example_traits));
        }

        let docs = self.doc(file, item.doc.as_ref());
        if let Some(docs) = &docs {
            traits.insert(smithy("documentation"), Json::String(docs.clone()));
        }
        let auth = if modifiers.contains("public") {
            traits.insert(smithy("auth"), Json::Array(Vec::new()));
            Vec::new()
        } else {
            default_auth.to_vec()
        };
        if modifiers.contains("internal") {
            traits.insert(smithy("internal"), Json::Object(JsonMap::new()));
        }
        let has_value =
            output_id.is_some() && (!response.body.is_empty() || response.payload.is_some());
        Some(IrOperation {
            response_protocol_headers: self.protocol.response_headers(&response, has_value),
            protocol_headers,
            internal: modifiers.contains("internal"),
            id,
            auth,
            public: modifiers.contains("public"),
            resource,
            docs,
            notes: annotations.notes,
            method,
            uri,
            status,
            input: input_id,
            output: output_id,
            errors,
            readonly,
            idempotent,
            idempotency_token,
            paginated,
            examples: lowered_examples,
            request,
            response,
            traits,
            requirements,
        })
    }

    // ---- service -----------------------------------------------------------

    fn service(&mut self, selected: Option<&str>) -> Option<Service> {
        let services: Vec<DeclRef<'a>> = self
            .decls
            .values()
            .filter(|decl| matches!(decl.item.decl, Decl::Service(_)) && decl.ns == self.ns)
            .copied()
            .collect();
        let chosen = match (selected, services.as_slice()) {
            (Some(name), _) => {
                let found = services
                    .iter()
                    .find(|d| decl_name(&d.item.decl).is_some_and(|n| n.name == name))
                    .copied();
                if found.is_none() {
                    self.package_error(format!("the package has no service `{name}`"));
                }
                found
            }
            (None, [only]) => Some(*only),
            (None, []) => {
                self.package_error("the package declares no service");
                None
            }
            (None, many) => {
                let names: Vec<String> = many
                    .iter()
                    .filter_map(|d| decl_name(&d.item.decl).map(|n| n.name.clone()))
                    .collect();
                self.package_error(format!(
                    "the package declares several services; choose one of {}",
                    names.join(", ")
                ));
                None
            }
        };
        let Some(decl) = chosen else {
            // Still check every declaration, so one run lists every problem.
            self.check_unreached(&BTreeSet::new(), &[], &JsonMap::new(), &[]);
            self.report_unplaced();
            self.shapes.clear();
            return None;
        };
        let file = decl.file;
        let item = decl.item;
        let Decl::Service(block) = &item.decl else {
            return None;
        };
        let id = self.id(&block.name.name);
        self.locate(id.clone(), file, item.span);
        self.service_name = block.name.name.clone();
        self.modifiers(file, item, &[]);
        let annotations =
            self.annotations(file, &item.annotations, &item.modifiers, Position::Service);
        let mut traits = annotations.traits;

        let mut version = None;
        let mut title = None;
        let mut protocol = None;
        let mut auth_scheme = None;
        let mut not_found = None;
        let mut pagination = JsonMap::new();
        let mut operation_names: Vec<&Ident> = Vec::new();
        let mut resource_names: Vec<&Ident> = Vec::new();
        let mut error_names: Vec<&Ident> = Vec::new();
        let mut clauses = BTreeSet::new();
        for clause in &block.clauses {
            let name = clause.name.name.as_str();
            if matches!(
                name,
                "version" | "title" | "protocol" | "auth" | "paginated" | "notFound"
            ) && !clauses.insert(name)
            {
                self.error(
                    file,
                    clause.span,
                    format!("a service has one `{name}` clause"),
                );
                continue;
            }
            match (clause.name.name.as_str(), &clause.value) {
                ("version", ClauseValue::Value(Value::Str(s))) => version = Some(s.value.clone()),
                ("title", ClauseValue::Value(Value::Str(s))) => title = Some(s.value.clone()),
                ("protocol", ClauseValue::Value(Value::Ident(p))) => protocol = Some(p),
                ("auth", ClauseValue::Value(Value::Ident(a))) => auth_scheme = Some(a),
                ("notFound", ClauseValue::Value(Value::Ident(e))) => not_found = Some(e),
                ("paginated", ClauseValue::Args(args)) => {
                    pagination = self.pagination_args(file, args);
                }
                (
                    "operations" | "resources" | "errors",
                    ClauseValue::Value(Value::List(values, _)),
                ) => {
                    let list = match clause.name.name.as_str() {
                        "operations" => &mut operation_names,
                        "resources" => &mut resource_names,
                        _ => &mut error_names,
                    };
                    for value in values {
                        match value {
                            Value::Ident(name) if list.iter().any(|n| n.name == name.name) => {
                                self.error(
                                    file,
                                    name.span,
                                    format!("`{}` is listed twice", name.name),
                                );
                            }
                            Value::Ident(name) => list.push(name),
                            other => self.error(file, other.span(), "expected a name"),
                        }
                    }
                }
                (name, _) => self.error(
                    file,
                    clause.span,
                    format!("unknown or malformed service clause `{name}`"),
                ),
            }
        }
        if version.is_none() {
            self.error(
                file,
                block.name.span,
                "a service declares its version: `version \"2026-01-01\"`",
            );
        }
        if let Some(name) = not_found {
            self.designate_not_found(file, name);
        }
        match protocol {
            Some(p) if p.name == "json" => self.protocol = Protocol::CrucibleJson,
            Some(p) => {
                self.error(
                    file,
                    p.span,
                    format!(
                        "protocol `{}` is not supported yet; use `protocol json`",
                        p.name
                    ),
                );
            }
            None => self.error(
                file,
                block.name.span,
                "a service declares its protocol: `protocol json`",
            ),
        }
        traits.insert(self.protocol.trait_id(), Json::Object(JsonMap::new()));
        let mut auth = Vec::new();
        let mut auth_schemes = BTreeMap::new();
        if let Some(scheme) = auth_scheme {
            if scheme.name == "bearer" {
                let scheme_id = smithy("httpBearerAuth");
                traits.insert(scheme_id.clone(), Json::Object(JsonMap::new()));
                auth.push(scheme_id.clone());
                auth_schemes.insert(scheme_id, AuthScheme::Bearer);
            } else {
                self.error(
                    file,
                    scheme.span,
                    format!(
                        "auth scheme `{}` is not supported yet; use `auth bearer`",
                        scheme.name
                    ),
                );
            }
        }
        if let Some(title) = &title {
            traits.insert(smithy("title"), Json::String(title.clone()));
        }
        if !pagination.is_empty() {
            traits.insert(smithy("paginated"), Json::Object(pagination.clone()));
        }
        let docs = self.doc(file, item.doc.as_ref());
        if let Some(docs) = &docs {
            traits.insert(smithy("documentation"), Json::String(docs.clone()));
        }

        // Which resources the service reaches: the roots it lists, and every
        // resource whose `in` chain leads to one of them.
        let roots: BTreeSet<String> = resource_names.iter().map(|r| r.name.clone()).collect();
        for root in &resource_names {
            match self.decl(&root.name).map(|d| &d.item.decl) {
                Some(Decl::Resource(Resource {
                    parent: Some(parent),
                    ..
                })) => self.error(
                    file,
                    root.span,
                    format!(
                        "`{}` is a child of `{}`; the service lists root resources only",
                        root.name, parent.name
                    ),
                ),
                Some(Decl::Resource(_)) => {}
                _ => self.error(
                    file,
                    root.span,
                    format!("`{}` is not a resource", root.name),
                ),
            }
        }
        let reachable = |lowerer: &Self, resource: &str| -> bool {
            let mut current = Some(resource.to_owned());
            let mut seen = BTreeSet::new();
            while let Some(name) = current {
                if roots.contains(&name) {
                    return true;
                }
                if !seen.insert(name.clone()) {
                    return false;
                }
                current = match lowerer.decl(&name).map(|d| &d.item.decl) {
                    Some(Decl::Resource(r)) => r.parent.as_ref().map(|p| p.name.clone()),
                    _ => None,
                };
            }
            false
        };
        let reached: BTreeSet<String> = self
            .decls
            .values()
            .filter_map(|decl| match &decl.item.decl {
                Decl::Resource(r) if reachable(self, &r.name.name) => Some(r.name.name.clone()),
                _ => None,
            })
            .collect();
        let permission_types = self.collect_permissions(&reached);
        let mut operations: Vec<DeclRef<'a>> = Vec::new();
        for name in &operation_names {
            match self.decl(&name.name) {
                Some(decl) if matches!(&decl.item.decl, Decl::Operation(o) if o.attachment.is_none()) => {
                    operations.push(decl);
                }
                Some(decl) if matches!(decl.item.decl, Decl::Operation(_)) => self.error(
                    file,
                    name.span,
                    format!("`{}` is attached to a resource; the service lists only operations bound to no resource", name.name),
                ),
                _ => self.error(file, name.span, format!("`{}` is not an operation", name.name)),
            }
        }
        let attached: Vec<DeclRef<'a>> = self
            .decls
            .values()
            .filter(|decl| match &decl.item.decl {
                Decl::Operation(o) => o
                    .attachment
                    .as_ref()
                    .is_some_and(|a| reachable(self, &a.resource.name)),
                _ => false,
            })
            .copied()
            .collect();
        let mut unreached = Vec::new();
        for decl in self.decls.values() {
            if let Decl::Operation(o) = &decl.item.decl
                && let Some(attachment) = &o.attachment
                && decl.ns == self.ns
                && !reachable(self, &attachment.resource.name)
            {
                unreached.push((
                    decl.file,
                    attachment.span,
                    format!(
                        "`{}` is not part of the service: the service does not reach `{}`; list it, or its root, in `resources`",
                        o.name.name, attachment.resource.name
                    ),
                ));
            }
        }
        for (file, span, message) in unreached {
            self.warning(file, span, message);
        }
        operations.extend(attached);

        let mut service_errors = Vec::new();
        for name in &error_names {
            match self.decl(&name.name).map(|d| &d.item.decl) {
                Some(Decl::Error(_)) => service_errors.push(self.ref_id(&name.name)),
                _ => self.error(file, name.span, format!("`{}` is not an error", name.name)),
            }
        }

        // Register every operation body, and which structures travel in HTTP
        // requests and responses, before any shape is lowered: member
        // timestamp formats depend on it.
        for decl in &operations {
            self.register_bodies(*decl);
        }
        self.http_outputs.extend(service_errors.iter().cloned());

        let mut lowered = Vec::new();
        for decl in &operations {
            if let Some(op) = self.operation(*decl, &service_errors, &pagination, &auth) {
                lowered.push(op);
            }
        }
        for error in &service_errors {
            self.visit(error);
        }
        lowered.sort_by(|a, b| a.id.name().cmp(b.id.name()));

        let version_declared = version.is_some();
        let mut service = Service {
            internal: false,
            recursive_groups: crucible_smithy::ir::recursive_groups(&self.shapes),
            protocol: self.protocol,
            errors: service_errors,
            protocol_settings: ProtocolSettings::default(),
            requirements: std::mem::take(&mut self.requirements).into_iter().collect(),
            id,
            version: version.unwrap_or_default(),
            title,
            docs,
            notes: annotations.notes,
            auth: auth.clone(),
            auth_schemes,
            operations: lowered,
            resources: BTreeMap::new(),
            resource_roots: Vec::new(),
            resource_references: BTreeMap::new(),
            // Cloned, not taken: `check_unreached` must not lower a reached
            // shape a second time and repeat its diagnostics.
            shapes: self.shapes.clone(),
            traits,
            renames: BTreeMap::new(),
            versions: Vec::new(),
            api_version: None,
            internal_traits: BTreeSet::new(),
            permissions: std::mem::take(&mut self.permissions),
            permission_types,
            not_found: self.not_found.clone(),
            exceptions: Vec::new(),
        };
        // A `.cru` service's version is a release its clients name in the
        // `Crucible-Version` header, when it can travel in one.
        service.api_version = Some(service.version.clone())
            .filter(|name| !name.is_empty() && name.bytes().all(|b| b.is_ascii_graphic()));
        // Absent is reported elsewhere; declared, it must travel in a header.
        if version_declared && service.api_version.is_none() {
            self.error_at(
                &service.id,
                "the service's version names the release clients send in the `Crucible-Version` header: use visible ASCII without spaces",
            );
        }
        // Rules of the IR itself, shared with the Smithy front end.
        let mut issues = crucible_smithy::Diagnostics::default();
        crucible_smithy::validate::service(&service, &mut issues);
        for issue in issues.iter() {
            self.error_at(&issue.shape, &issue.message);
        }
        self.check_permissions(&service);
        // Unreached operations attach `NotFoundException` against the
        // service's permissions too.
        self.permissions = service.permissions.clone();
        let included: BTreeSet<&str> = operations
            .iter()
            .filter_map(|decl| decl_name(&decl.item.decl).map(|n| n.name.as_str()))
            .collect();
        // Operations the service does not reach are checked in its context:
        // its errors, default pagination and auth, and the shared rules.
        let errors = service.errors.clone();
        let unreached = self.check_unreached(&included, &errors, &pagination, &auth);
        // Every structure is lowered now, so which are used as types is known.
        self.report_unplaced();
        let mut probe = service.clone();
        probe.operations = unreached;
        probe.shapes = std::mem::take(&mut self.shapes);
        self.check_unreached_permissions(&probe);
        let mut issues = crucible_smithy::Diagnostics::default();
        for operation in &probe.operations {
            crucible_smithy::validate::operation(&probe, operation, &mut issues);
        }
        for issue in issues.iter() {
            self.error_at(&issue.shape, &issue.message);
        }
        service.project_metadata_only();
        Some(service)
    }

    /// Lowers what the service does not reach, only for its diagnostics: a
    /// declaration nothing uses yet must still be a valid one. None of it
    /// enters the IR.
    fn check_unreached(
        &mut self,
        included: &BTreeSet<&str>,
        errors: &[ShapeId],
        pagination: &JsonMap<String, Json>,
        auth: &[ShapeId],
    ) -> Vec<IrOperation> {
        let decls: Vec<DeclRef<'a>> = self.decls.values().copied().collect();
        let operations: Vec<DeclRef<'a>> = decls
            .iter()
            .filter(|decl| {
                matches!(decl.item.decl, Decl::Operation(_))
                    && decl_name(&decl.item.decl)
                        .is_some_and(|n| !included.contains(n.name.as_str()))
            })
            .copied()
            .collect();
        for decl in &operations {
            self.register_bodies(*decl);
        }
        let mut lowered = Vec::new();
        for decl in operations {
            lowered.extend(self.operation(decl, errors, pagination, auth));
        }
        for decl in decls {
            if is_type(&decl.item.decl)
                && let Some(name) = decl_name(&decl.item.decl)
            {
                let id = self.ref_id(&name.name);
                self.visit(&id);
            }
        }
        lowered
    }

    /// Records an operation's bodies under their generated names, and which
    /// structures travel in HTTP requests and responses.
    fn register_bodies(&mut self, decl: DeclRef<'a>) {
        let Decl::Operation(op) = &decl.item.decl else {
            return;
        };
        let labels: Vec<String> = op
            .entries
            .iter()
            .find_map(|entry| match entry {
                OpEntry::Http(http) => Some(
                    http.uri
                        .value
                        .split('?')
                        .next()
                        .unwrap_or_default()
                        .split('/')
                        .filter_map(|part| part.strip_prefix('{').and_then(|p| p.strip_suffix('}')))
                        .map(|label| label.trim_end_matches('+').to_owned())
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default();
        for entry in &op.entries {
            match entry {
                OpEntry::Body(body) => {
                    let name = match body.kind {
                        BodyKind::Input => self.naming.input(&op.name.name),
                        BodyKind::Output => self.naming.output(&op.name.name),
                    };
                    let body_id = ShapeId::new(self.ns, &name);
                    match body.kind {
                        BodyKind::Input => self.http_inputs.insert(body_id.clone()),
                        BodyKind::Output => self.http_outputs.insert(body_id.clone()),
                    };
                    if self.decls.contains_key(name.as_str()) {
                        self.error(
                            decl.file,
                            body.span,
                            format!("`{name}` is the generated name of this body and is already declared"),
                        );
                    }
                    self.bodies.insert(
                        body_id,
                        BodySource {
                            file: decl.file,
                            operation: op,
                            body,
                            labels: labels.clone(),
                        },
                    );
                }
                OpEntry::Errors(errors, _) => {
                    for error in errors {
                        let id = self.ref_id(&error.name);
                        self.http_outputs.insert(id);
                    }
                }
                _ => {}
            }
        }
    }
}

/// The `[Name]` shortcut links in Markdown text: a capitalised identifier in
/// brackets, outside code spans, not part of an inline `[text](url)` or
/// reference `[text][ref]` link or a `[ref]: url` definition.
fn doc_links(text: &str) -> Vec<&str> {
    let mut links = Vec::new();
    for line in text.lines() {
        let bytes = line.as_bytes();
        let mut code = false;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'`' => code = !code,
                b'[' if !code && (i == 0 || bytes[i - 1] != b']') => {
                    if let Some(len) = line[i + 1..].find(']') {
                        let name = &line[i + 1..i + 1 + len];
                        let after = bytes.get(i + 2 + len).copied();
                        let identifier = name.starts_with(|c: char| c.is_ascii_uppercase())
                            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                        if identifier && !matches!(after, Some(b'(' | b'[' | b':')) {
                            links.push(name);
                        }
                        i += len + 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
    links
}

/// What a constraint would constrain.
#[derive(Clone, Copy, PartialEq, Eq)]
enum On {
    Text,
    StringEnum,
    Blob,
    Number,
    Timestamp,
    List,
    Map,
    Other,
}

fn on_kind(kind: SimpleKind) -> On {
    match kind {
        SimpleKind::String => On::Text,
        SimpleKind::Blob => On::Blob,
        SimpleKind::Timestamp => On::Timestamp,
        SimpleKind::Byte
        | SimpleKind::Short
        | SimpleKind::Integer
        | SimpleKind::Long
        | SimpleKind::Float
        | SimpleKind::Double
        | SimpleKind::BigInteger
        | SimpleKind::BigDecimal => On::Number,
        SimpleKind::Boolean | SimpleKind::Document => On::Other,
    }
}

/// Whether the bounds `[min, max]` lie inside `[outer_min, outer_max]`;
/// a missing bound is unbounded.
fn within<T>(
    min: Option<T>,
    max: Option<T>,
    outer_min: Option<T>,
    outer_max: Option<T>,
    cmp: impl Fn(&T, &T) -> std::cmp::Ordering,
) -> bool {
    let low = match (&min, &outer_min) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(min), Some(outer)) => cmp(min, outer).is_ge(),
    };
    let high = match (&max, &outer_max) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(max), Some(outer)) => cmp(max, outer).is_le(),
    };
    low && high
}

/// `a` or `an` before a declaration keyword: `an error`, `a structure`.
fn article(word: &str) -> &'static str {
    if word.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    }
}
