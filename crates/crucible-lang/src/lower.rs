//! Lowering a parsed package to `ir::Service`.
//!
//! The IR is shared with the Smithy front end, so the semantics it settles
//! (bindings, effective constraints, timestamp formats, error envelopes)
//! must come out the same from both. Lowering works with the Smithy-equivalent
//! traits internally; the finished IR keeps only project metadata in its raw
//! `traits` maps, because every generator reads typed fields (decision 63).
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
    Annotation, Body, BodyKind, ClauseValue, Constraint, ConstraintValue, Decl, Doc, EnumValue,
    ErrorEntry, ExamplePart, Ident, Item, Key, Member, MemberKind, MetaType, Metadata, NumLit,
    OpEntry, Operation, Resource, ResourceEntry, TypeRef, Value,
};
use crate::compile::{FileDiagnostic, Location, Naming, Package, SourceMap};
use crate::diagnostic::Diagnostic;
use crate::span::Span;

/// `namespaces[i]` is the namespace of `parsed[i]`: the package's own, or a
/// bundled import's.
pub(crate) fn lower(
    package: &Package,
    parsed: &[Parsed],
    namespaces: &[String],
    service: Option<&str>,
) -> (Option<Service>, SourceMap, Vec<FileDiagnostic>) {
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
        visiting: BTreeSet::new(),
        recursive: BTreeSet::new(),
        requirements: BTreeSet::new(),
        http_inputs: BTreeSet::new(),
        http_outputs: BTreeSet::new(),
    };
    lowerer.collect(parsed, namespaces);
    let service = lowerer.service(service);
    lowerer.report_recursion();
    (service, lowerer.source_map, lowerer.diagnostics)
}

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
    union: bool,
    /// URI labels, for an operation input: a member of that name binds to it.
    labels: &'a [String],
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

/// Whether a trait id is one of Smithy's or Crucible's own, which the IR
/// already represents as typed fields.
fn is_standard_trait(id: &ShapeId) -> bool {
    id.is_prelude()
        || id.namespace().starts_with("aws.")
        || id.namespace() == "crucible.protocols"
        || id.namespace() == "smithy.test"
}

/// The IR's raw `traits` maps carry project metadata only (decision 43).
/// Lowering uses the Smithy-equivalent traits internally, for bindings and
/// timestamp formats; every typed fact they encode is already on the IR.
fn project_metadata_only(service: &mut Service) {
    let strip = |traits: &mut Traits| traits.retain(|id, _| !is_standard_trait(id));
    strip(&mut service.traits);
    for operation in &mut service.operations {
        strip(&mut operation.traits);
    }
    for shape in service.shapes.values_mut() {
        match shape {
            Shape::Unit { .. } => {}
            Shape::Simple(s) => strip(&mut s.traits),
            Shape::Enum(s) => strip(&mut s.traits),
            Shape::IntEnum(s) => strip(&mut s.traits),
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
    visiting: BTreeSet<ShapeId>,
    recursive: BTreeSet<ShapeId>,
    requirements: BTreeSet<FeatureUse>,
    http_inputs: BTreeSet<ShapeId>,
    http_outputs: BTreeSet<ShapeId>,
}

impl<'a> Lowerer<'a> {
    // ---- bookkeeping -------------------------------------------------------

    fn error(&mut self, file: usize, span: Span, message: impl Into<String>) {
        self.diagnostics.push(FileDiagnostic {
            file: Some(file),
            diagnostic: Diagnostic::new(span, message),
        });
    }

    fn package_error(&mut self, message: impl Into<String>) {
        self.diagnostics.push(FileDiagnostic {
            file: None,
            diagnostic: Diagnostic::new(Span::default(), message),
        });
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
                    Decl::Rule(_)
                    | Decl::Template(_)
                    | Decl::Except(_)
                    | Decl::Migration(_)
                    | Decl::Protocol(_)
                    | Decl::Conversion(_) => {
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

    fn report_recursion(&mut self) {
        let recursive = std::mem::take(&mut self.recursive);
        for id in recursive {
            if let Some(decl) = self.decl(id.name()) {
                self.error(
                    decl.file,
                    decl.item.span,
                    format!("`{}` refers to itself; recursive shapes are not supported by the compiler yet", id.name()),
                );
            }
        }
    }

    // ---- names and values --------------------------------------------------

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
                    let key = match &field.key {
                        Key::Name(name) => name.name.clone(),
                        Key::Text(text) => text.value.clone(),
                    };
                    let value = self.json(file, &field.value);
                    map.insert(key, value);
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
        position: Position,
    ) -> Annotations {
        let mut out = Annotations::default();
        let mut custom: Vec<(&Ident, &Metadata)> = Vec::new();
        for annotation in annotations {
            let name = annotation.name.name.as_str();
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
                    let source_only = match &metadata.retention {
                        None => false,
                        Some(r) if r.name == "runtime" => false,
                        Some(r) if r.name == "source" => true,
                        Some(r) => {
                            self.error(decl.file, r.span, "retention is `source` or `runtime`");
                            false
                        }
                    };
                    // Source-only metadata is for the compiler and rules; the
                    // runtime never sees it (decision 43).
                    if !source_only {
                        out.traits.insert(self.ref_id(name), value);
                    }
                    custom.push((&annotation.name, metadata));
                }
            }
        }
        let present: BTreeSet<&str> = annotations.iter().map(|a| a.name.name.as_str()).collect();
        for (name, metadata) in custom {
            for conflict in &metadata.conflicts {
                if present.contains(conflict.name.as_str()) {
                    self.error(
                        file,
                        name.span,
                        format!("`@{}` conflicts with `@{}`", name.name, conflict.name),
                    );
                }
            }
        }
        out
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
                if ty.name.name == "string" && !matches!(arg.value, Value::Str(_)) {
                    self.error(file, arg.value.span(), format!("`@{name}` takes a string"));
                }
                self.json(file, &arg.value)
            }
            (Some(_), args) => {
                let mut map = JsonMap::new();
                for arg in args {
                    match &arg.key {
                        Some(Key::Name(key)) => {
                            let value = self.json(file, &arg.value);
                            map.insert(key.name.clone(), value);
                        }
                        _ => self.error(
                            file,
                            arg.value.span(),
                            format!("`@{name}` takes one value or `key: value` pairs"),
                        ),
                    }
                }
                Json::Object(map)
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
        if self.visiting.contains(id) {
            self.recursive.insert(id.clone());
            return;
        }
        self.visiting.insert(id.clone());
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
            streaming: false,
            requires_length: false,
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
        let annotations = self.annotations(file, &item.annotations, Position::Shape(keyword));
        let mut traits = annotations.traits;
        let docs = doc_text(item.doc.as_ref());
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
                if info.format.is_some() && kind != SimpleKind::Timestamp {
                    self.error(file, scalar.name.span, "only a timestamp has a `format`");
                }
                if info.media_type.is_some()
                    && !matches!(kind, SimpleKind::Blob | SimpleKind::String)
                {
                    self.error(
                        file,
                        scalar.name.span,
                        "only a blob or string has a `mediaType`",
                    );
                }
                traits.extend(info.traits);
                Shape::Simple(Simple {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    streaming: false,
                    requires_length: false,
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
                if integer {
                    self.require(id, Feature::IntEnum, smithy("IntEnum"));
                    let values = enumeration
                        .values
                        .iter()
                        .filter_map(|value| self.int_enum_value(file, value))
                        .collect();
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
                        let lowered = self.enum_value(file, value);
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
                let modifiers = self.modifiers(file, item, &["internal", "sensitive"]);
                let (mut traits, notes, docs) = self.shape_traits(file, item, "list", &modifiers);
                let target = self.resolve_type(file, &list.member)?;
                let member = self.collection_member(id, "member", &target);
                let info = self.constraint_block(file, &list.constraints);
                traits.extend(info.traits);
                Shape::List(List {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    sparse: false,
                    is_set: false,
                    member,
                    constraints: info.constraints,
                    docs,
                    notes,
                    traits,
                })
            }
            Decl::Map(map) => {
                let modifiers = self.modifiers(file, item, &["internal", "sensitive"]);
                let (mut traits, notes, docs) = self.shape_traits(file, item, "map", &modifiers);
                let key = self.resolve_type(file, &map.key)?;
                let value = self.resolve_type(file, &map.value)?;
                let key = self.collection_member(id, "key", &key);
                let value = self.collection_member(id, "value", &value);
                let info = self.constraint_block(file, &map.constraints);
                traits.extend(info.traits);
                Shape::Map(Map {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    sparse: false,
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
                let context = MemberContext {
                    owner: id,
                    resource,
                    union: false,
                    labels: &[],
                };
                let members = self.members(file, &structure.members, context);
                Shape::Structure(IrStructure {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    streaming: false,
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
                    union: true,
                    labels: &[],
                };
                let members = self.members(file, &union.members, context);
                Shape::Union(IrStructure {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    streaming: false,
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
                    union: false,
                    labels: &[],
                };
                let members = self.members(file, &members, context);
                let info = self.error_info(id, &members, fault, status, retryable);
                Shape::Structure(IrStructure {
                    internal: modifiers.contains("internal"),
                    sensitive: modifiers.contains("sensitive"),
                    streaming: false,
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

    fn enum_value(&mut self, file: usize, value: &EnumValue) -> IrEnumValue {
        let annotations = self.annotations(file, &value.annotations, Position::EnumValue);
        let wire = match &value.value {
            None => value.name.name.clone(),
            Some(Value::Str(s)) => s.value.clone(),
            Some(other) => {
                self.error(file, other.span(), "a string enum value is a string");
                value.name.name.clone()
            }
        };
        IrEnumValue {
            internal: false,
            name: value.name.name.clone(),
            value: wire,
            docs: doc_text(value.doc.as_ref()),
            notes: annotations.notes,
        }
    }

    fn int_enum_value(&mut self, file: usize, value: &EnumValue) -> Option<IntEnumValue> {
        let annotations = self.annotations(file, &value.annotations, Position::EnumValue);
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
        Some(IntEnumValue {
            internal: false,
            name: value.name.name.clone(),
            value: number,
            docs: doc_text(value.doc.as_ref()),
            notes: annotations.notes,
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
        let resource = source
            .operation
            .attachment
            .as_ref()
            .map(|a| a.resource.name.as_str());
        let labels: &[String] = if source.body.kind == BodyKind::Input {
            &source.labels
        } else {
            &[]
        };
        let context = MemberContext {
            owner: id,
            resource,
            union: false,
            labels,
        };
        let members = self.members(file, &source.body.members, context);
        Shape::Structure(IrStructure {
            internal: false,
            sensitive: false,
            streaming: false,
            strict,
            input: source.body.kind == BodyKind::Input,
            members,
            error: None,
            docs: None,
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
            event_binding: None,
            name: name.to_owned(),
            wire_name: name.to_owned(),
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
        match self.shapes.get(id) {
            Some(Shape::Simple(s)) => s.constraints.clone(),
            Some(Shape::List(l)) => l.constraints.clone(),
            Some(Shape::Map(m)) => m.constraints.clone(),
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
        self.members_into(file, members, context, &mut out, &mut including);
        let mut names = BTreeSet::new();
        for member in &out {
            if !names.insert(member.name.clone()) {
                if let Some(decl) = self.decl(context.owner.name()) {
                    self.error(
                        decl.file,
                        decl.item.span,
                        format!(
                            "member `{}` is declared twice in `{}`",
                            member.name,
                            context.owner.name()
                        ),
                    );
                } else {
                    self.package_error(format!(
                        "member `{}` is declared twice in `{}`",
                        member.name,
                        context.owner.name()
                    ));
                }
            }
        }
        out
    }

    fn members_into(
        &mut self,
        file: usize,
        members: &[Member],
        context: MemberContext<'_>,
        out: &mut Vec<IrMember>,
        including: &mut Vec<String>,
    ) {
        for member in members {
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
                self.members_into(decl.file, &structure.members, context, out, including);
                including.pop();
                continue;
            }
            if let Some(lowered) = self.member(file, member, context) {
                out.push(lowered);
            }
        }
    }

    /// The identifier or property `name` of `resource`, searching parents.
    fn resource_entry(&self, resource: &str, name: &str) -> Option<(usize, &'a ResourceEntry)> {
        let mut current = Some(resource.to_owned());
        let mut seen = BTreeSet::new();
        while let Some(resource) = current {
            if !seen.insert(resource.clone()) {
                return None;
            }
            let decl = self.decl(&resource)?;
            let Decl::Resource(r) = &decl.item.decl else {
                return None;
            };
            if let Some(entry) = r.entries.iter().find(|entry| entry.name.name == name) {
                return Some((decl.file, entry));
            }
            current = r.parent.as_ref().map(|parent| parent.name.clone());
        }
        None
    }

    fn member(
        &mut self,
        file: usize,
        member: &Member,
        context: MemberContext<'_>,
    ) -> Option<IrMember> {
        let (name, name_span, ty, ty_file, required, default, doc) = match &member.kind {
            MemberKind::Include(_) => return None,
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
                let Some((entry_file, entry)) = self.resource_entry(resource, &name.name) else {
                    self.error(
                        file,
                        name.span,
                        format!(
                            "resource `{resource}` has no identifier or property `{}`",
                            name.name
                        ),
                    );
                    return None;
                };
                let member_name = alias.as_ref().unwrap_or(name);
                let doc = member.doc.as_ref().or(entry.doc.as_ref());
                (
                    member_name.name.clone(),
                    member_name.span,
                    Some(&entry.ty),
                    entry_file,
                    !optional,
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
        let annotations = self.annotations(file, &member.annotations, Position::Member);
        let target = match ty {
            Some(ty) => self.resolve_type(ty_file, &ty.name)?,
            None => smithy("Unit"),
        };
        self.visit(&target);
        let own = match ty {
            Some(TypeRef { constraints, .. }) => self.constraint_block(ty_file, constraints),
            None => ConstraintInfo::default(),
        };
        let mut traits = annotations.traits;
        traits.extend(own.traits);
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
            if !required {
                self.error(
                    file,
                    name_span,
                    format!("`{name}` binds the URI label {{{name}}}, so it must be required"),
                );
            }
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
        let default = default.map(|value| self.json(file, value));
        if let Some(default) = &default {
            traits.insert(smithy("default"), default.clone());
            if matches!(self.shapes.get(&target), Some(Shape::Simple(s)) if s.kind == SimpleKind::Timestamp)
            {
                self.error(
                    file,
                    name_span,
                    "timestamp defaults are not supported by the compiler yet",
                );
            }
        }
        let docs = doc_text(doc);
        if let Some(docs) = &docs {
            traits.insert(smithy("documentation"), Json::String(docs.clone()));
        }
        let mut constraints = self.constraints_of(&target);
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
            internal: false,
            sensitive: false,
            event_binding: None,
            name: name.clone(),
            wire_name: name,
            target,
            required,
            wire_default: default.clone(),
            default,
            // Decision 20: a required member is required in generated clients too.
            client_optional: false,
            idempotency_token: annotations.token,
            mutability: annotations.mutability,
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
                    self.package_error(format!("{owner}: only one member may be the payload"));
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
                bindings.headers.push(Binding {
                    member: member.name.clone(),
                    name: header,
                });
            } else {
                bindings.body.push(member.name.clone());
            }
        }
        let mut headers = BTreeSet::new();
        for binding in &bindings.headers {
            if !headers.insert(binding.name.to_ascii_lowercase()) {
                self.package_error(format!("{owner}: duplicate HTTP header {:?}", binding.name));
            }
        }
        let mut query = BTreeSet::new();
        for binding in &bindings.query {
            if !query.insert(binding.name.clone()) {
                self.package_error(format!(
                    "{owner}: duplicate query parameter {:?}",
                    binding.name
                ));
            }
        }
        if let Some(payload) = &bindings.payload {
            for member in &bindings.body {
                self.package_error(format!(
                    "{owner}: the body is the payload `{}`; bind `{member}` to a label, query parameter or header",
                    payload.member
                ));
            }
        }
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
                    self.package_error(format!(
                        "{owner}${}: a payload is a blob, string, structure, union or document",
                        member.name
                    ));
                    return None;
                }
            },
            Some(Shape::Enum(e)) => (PayloadKind::String, &e.traits),
            Some(Shape::Structure(s) | Shape::Union(s)) => (PayloadKind::Document, &s.traits),
            _ => {
                self.package_error(format!(
                    "{owner}${}: a payload is a blob, string, structure, union or document",
                    member.name
                ));
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
                    PayloadKind::EventStream => "application/vnd.amazon.eventstream",
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
            streaming: false,
            requires_length: false,
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
            prefix_headers: bindings.prefix_headers,
            response_code: bindings.response_code,
        }
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
            match part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
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
        let annotations = self.annotations(file, &item.annotations, Position::Operation);
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
                OpEntry::Errors(errors, _) => own_errors.extend(errors),
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
            }
        }

        let Some(http) = http else {
            self.error(
                file,
                operation.name.span,
                "an operation served over HTTP needs an `http METHOD \"/uri\"` clause",
            );
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
                if !ROLES.contains(&attachment.role.name.as_str()) {
                    self.error(file, attachment.role.span, "unknown role");
                }
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
                        MemberKind::Include(_) => None,
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
        let idempotent = !readonly
            && (modifiers.contains("idempotent")
                || matches!(role, Some("puts" | "updates" | "deletes"))
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

        let mut protocol_headers = self
            .protocol
            .request_headers(&self.service_name, &operation.name.name);
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
        for error in service_errors {
            if !errors.contains(error) {
                errors.push(error.clone());
            }
        }
        for error in &errors {
            self.visit(error);
        }

        let paginated = pagination.and_then(|call| {
            let mut own = JsonMap::new();
            for arg in &call.args {
                match (&arg.key, &arg.value) {
                    (Some(Key::Name(key)), Value::Ident(member)) => {
                        own.insert(key.name.clone(), Json::String(member.name.clone()));
                    }
                    _ => self.error(
                        file,
                        arg.value.span(),
                        "`paginated` takes `inputToken:`, `outputToken:`, `items:` and `pageSize:` member names",
                    ),
                }
            }
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
            for (what, member, members) in [
                ("inputToken", Some(&paginated.input_token), &input_members),
                ("pageSize", paginated.page_size.as_ref(), &input_members),
                ("outputToken", Some(&paginated.output_token), &output_members),
                ("items", paginated.items.as_ref(), &output_members),
            ] {
                if let Some(member) = member
                    && !members.iter().any(|m| &m.name == member)
                {
                    self.error(
                        file,
                        call.span,
                        format!("pagination {what} `{member}` is not a member of the operation's {}", if matches!(what, "inputToken" | "pageSize") { "input" } else { "output" }),
                    );
                }
            }
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
                docs: doc_text(example.doc.as_ref()),
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
            for part in &example.parts {
                match part {
                    ExamplePart::Input(value) => lowered.input = Some(self.json(file, value)),
                    ExamplePart::Output(value) => lowered.output = Some(self.json(file, value)),
                    ExamplePart::Error(name, value) => {
                        let shape = self.ref_id(&name.name);
                        if !errors.contains(&shape) {
                            self.error(
                                file,
                                name.span,
                                format!("`{}` is not one of this operation's errors", name.name),
                            );
                        }
                        let content = self.json(file, value);
                        lowered.error = Some(ExampleError { shape, content });
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

        let docs = doc_text(item.doc.as_ref());
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
        let has_value = output_id.is_some()
            && (!response.body.is_empty()
                || response.payload.is_some()
                || response.event_stream.is_some());
        Some(IrOperation {
            response_protocol_headers: self.protocol.response_headers(&response, has_value),
            protocol_headers,
            internal: modifiers.contains("internal"),
            unwrapped_xml_output: false,
            unsigned_payload: false,
            discovered_endpoint: None,
            checksum: None,
            id,
            auth,
            scopes: Vec::new(),
            optional_auth: false,
            endpoint: None,
            endpoint_parameters: BTreeMap::new(),
            waiters: BTreeMap::new(),
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
            self.check_unreached(&BTreeSet::new(), &[]);
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
        let annotations = self.annotations(file, &item.annotations, Position::Service);
        let mut traits = annotations.traits;

        let mut version = None;
        let mut title = None;
        let mut protocol = None;
        let mut auth_scheme = None;
        let mut pagination = JsonMap::new();
        let mut operation_names: Vec<&Ident> = Vec::new();
        let mut resource_names: Vec<&Ident> = Vec::new();
        let mut error_names: Vec<&Ident> = Vec::new();
        for clause in &block.clauses {
            match (clause.name.name.as_str(), &clause.value) {
                ("version", ClauseValue::Value(Value::Str(s))) => version = Some(s.value.clone()),
                ("title", ClauseValue::Value(Value::Str(s))) => title = Some(s.value.clone()),
                ("protocol", ClauseValue::Value(Value::Ident(p))) => protocol = Some(p),
                ("auth", ClauseValue::Value(Value::Ident(a))) => auth_scheme = Some(a),
                ("paginated", ClauseValue::Args(args)) => {
                    for arg in args {
                        match (&arg.key, &arg.value) {
                            (Some(Key::Name(key)), Value::Ident(member)) => {
                                pagination
                                    .insert(key.name.clone(), Json::String(member.name.clone()));
                            }
                            _ => self.error(
                                file,
                                arg.value.span(),
                                "`paginated` takes `key: member` pairs",
                            ),
                        }
                    }
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
        let docs = doc_text(item.doc.as_ref());
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

        let mut service = Service {
            cors: None,
            internal: false,
            endpoint_discovery: None,
            recursive_groups: Vec::new(),
            xml_namespace: None,
            xml_shapes: BTreeMap::new(),
            protocol: self.protocol,
            protocols: vec![self.protocol],
            errors: service_errors,
            protocol_settings: ProtocolSettings::default(),
            requirements: std::mem::take(&mut self.requirements).into_iter().collect(),
            id,
            version: version.unwrap_or_default(),
            title,
            docs,
            notes: annotations.notes,
            auth: auth.clone(),
            scopes: Vec::new(),
            auth_schemes,
            endpoint_rules: None,
            protocol_tests: Vec::new(),
            operations: lowered,
            resources: BTreeMap::new(),
            resource_roots: Vec::new(),
            resource_references: BTreeMap::new(),
            shapes: std::mem::take(&mut self.shapes),
            traits,
            renames: BTreeMap::new(),
        };
        let included: BTreeSet<&str> = operations
            .iter()
            .filter_map(|decl| decl_name(&decl.item.decl).map(|n| n.name.as_str()))
            .collect();
        self.check_unreached(&included, &auth);
        project_metadata_only(&mut service);
        Some(service)
    }

    /// Lowers what the service does not reach, only for its diagnostics: a
    /// declaration nothing uses yet must still be a valid one. None of it
    /// enters the IR.
    fn check_unreached(&mut self, included: &BTreeSet<&str>, auth: &[ShapeId]) {
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
        for decl in operations {
            self.operation(decl, &[], &JsonMap::new(), auth);
        }
        for decl in decls {
            if is_type(&decl.item.decl)
                && let Some(name) = decl_name(&decl.item.decl)
            {
                let id = self.ref_id(&name.name);
                self.visit(&id);
            }
        }
        self.shapes.clear();
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

/// `a` or `an` before a declaration keyword: `an error`, `a structure`.
fn article(word: &str) -> &'static str {
    if word.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    }
}
