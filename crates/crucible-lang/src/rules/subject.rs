//! The elements a rule ranges over, taken from the lowered model, with the
//! few facts only the syntax tree has (a role, a resource's parent, a shape
//! nothing reaches). Only the compiled package's own elements are subjects:
//! an imported package's shapes are checked where that package is compiled.

use std::collections::{BTreeMap, BTreeSet};

use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Constraints, Member, Operation, Service, Shape, SimpleKind, Traits};

use super::Inputs;
use super::vocabulary::Kind;
use crate::ast::{Decl, Item, MemberKind};
use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    Input,
    Output,
}

/// What a named shape is, from the lowered shape or, for one no operation
/// reaches, from its declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    Error,
    Structure,
    Union,
    StringEnum,
    IntEnum,
    List,
    Map,
    Simple(SimpleKind),
}

impl Class {
    pub(super) fn of(shape: &Shape) -> Option<Class> {
        Some(match shape {
            Shape::Unit { .. } => return None,
            Shape::Simple(s) => Class::Simple(s.kind),
            Shape::Enum(_) => Class::StringEnum,
            Shape::IntEnum(_) => Class::IntEnum,
            Shape::List(_) => Class::List,
            Shape::Map(_) => Class::Map,
            Shape::Union(_) => Class::Union,
            Shape::Structure(s) if s.error.is_some() => Class::Error,
            Shape::Structure(_) => Class::Structure,
        })
    }

    /// Whether a shape of this class is what `kind` ranges over.
    pub(super) fn is(self, kind: Kind) -> bool {
        use SimpleKind as S;
        matches!(
            (kind, self),
            (Kind::Shape, _)
                | (Kind::Error, Class::Error)
                | (Kind::Structure, Class::Structure)
                | (Kind::Union, Class::Union)
                | (Kind::List, Class::List)
                | (Kind::Map, Class::Map)
                | (Kind::Enum, Class::StringEnum | Class::IntEnum)
                | (Kind::String, Class::StringEnum | Class::Simple(S::String))
                | (
                    Kind::Number,
                    Class::IntEnum
                        | Class::Simple(
                            S::Byte
                                | S::Short
                                | S::Integer
                                | S::Long
                                | S::Float
                                | S::Double
                                | S::BigInteger
                                | S::BigDecimal
                        ),
                )
                | (Kind::Boolean, Class::Simple(S::Boolean))
                | (Kind::Byte, Class::Simple(S::Byte))
                | (Kind::Short, Class::Simple(S::Short))
                | (Kind::Integer, Class::Simple(S::Integer))
                | (Kind::Long, Class::Simple(S::Long))
                | (Kind::Float, Class::Simple(S::Float))
                | (Kind::Double, Class::Simple(S::Double))
                | (Kind::BigInteger, Class::Simple(S::BigInteger))
                | (Kind::BigDecimal, Class::Simple(S::BigDecimal))
                | (Kind::Timestamp, Class::Simple(S::Timestamp))
                | (Kind::Blob, Class::Simple(S::Blob))
                | (Kind::Document, Class::Simple(S::Document))
        )
    }

    pub(super) fn is_enum(self) -> bool {
        matches!(self, Class::StringEnum | Class::IntEnum)
    }
}

/// What the rules need of any shape.
pub(super) struct ShapeParts<'a> {
    pub docs: Option<&'a str>,
    pub traits: Option<&'a Traits>,
    pub internal: bool,
    pub sensitive: bool,
    pub constraints: Option<&'a Constraints>,
}

pub(super) fn parts(shape: &Shape) -> ShapeParts<'_> {
    match shape {
        Shape::Unit { internal } => ShapeParts {
            docs: None,
            traits: None,
            internal: *internal,
            sensitive: false,
            constraints: None,
        },
        Shape::Simple(s) => ShapeParts {
            docs: s.docs.as_deref(),
            traits: Some(&s.traits),
            internal: s.internal,
            sensitive: s.sensitive,
            constraints: Some(&s.constraints),
        },
        Shape::Enum(s) => ShapeParts {
            docs: s.docs.as_deref(),
            traits: Some(&s.traits),
            internal: s.internal,
            sensitive: s.sensitive,
            constraints: None,
        },
        Shape::IntEnum(s) => ShapeParts {
            docs: s.docs.as_deref(),
            traits: Some(&s.traits),
            internal: s.internal,
            sensitive: s.sensitive,
            constraints: None,
        },
        Shape::List(s) => ShapeParts {
            docs: s.docs.as_deref(),
            traits: Some(&s.traits),
            internal: s.internal,
            sensitive: s.sensitive,
            constraints: Some(&s.constraints),
        },
        Shape::Map(s) => ShapeParts {
            docs: s.docs.as_deref(),
            traits: Some(&s.traits),
            internal: s.internal,
            sensitive: s.sensitive,
            constraints: Some(&s.constraints),
        },
        Shape::Structure(s) | Shape::Union(s) => ShapeParts {
            docs: s.docs.as_deref(),
            traits: Some(&s.traits),
            internal: s.internal,
            sensitive: s.sensitive,
            constraints: None,
        },
    }
}

pub(super) enum Entity<'a> {
    Service(&'a Service),
    Resource {
        documented: bool,
    },
    Operation(&'a Operation),
    Shape {
        /// `None` for a shape no operation reaches, which was never lowered.
        shape: Option<&'a Shape>,
        class: Class,
        documented: bool,
        referenced: bool,
    },
    Member {
        member: &'a Member,
        side: Option<Side>,
        template: bool,
    },
}

pub(super) struct Subject<'a> {
    pub entity: Entity<'a>,
    /// What the subject is called: a shape, operation or resource name, or a
    /// member's.
    pub name: String,
    /// What is shown in a diagnostic: the name, or `Shape.member`.
    pub display: String,
    /// The other names an exception may give: a member's shape, and for an
    /// input or output member its operation.
    pub aliases: Vec<String>,
    pub id: ShapeId,
    /// Where it is declared when the source map does not say.
    pub declared: Option<(usize, Span)>,
}

/// What the rules know beyond the subjects themselves.
pub(super) struct World<'a> {
    pub package: &'a str,
    pub service: &'a Service,
    roles: BTreeMap<String, String>,
    parents: BTreeMap<String, Option<String>>,
    pub subjects: Vec<Subject<'a>>,
}

impl<'a> World<'a> {
    pub(super) fn new(inputs: &Inputs<'a>) -> World<'a> {
        let package = inputs.package;
        let own = |file: usize| inputs.namespaces.get(file).is_some_and(|ns| ns == package);
        let mut roles = BTreeMap::new();
        let mut parents = BTreeMap::new();
        let mut declared: BTreeMap<String, (usize, &Item)> = BTreeMap::new();
        for (file, parsed) in inputs.parsed.iter().enumerate() {
            for item in &parsed.file.items {
                match &item.decl {
                    Decl::Operation(op) if own(file) => {
                        if let Some(attachment) = &op.attachment {
                            roles.insert(op.name.name.clone(), attachment.role.name.clone());
                        }
                    }
                    Decl::Resource(resource) => {
                        parents.insert(
                            resource.name.name.clone(),
                            resource.parent.as_ref().map(|p| p.name.clone()),
                        );
                        if own(file) {
                            declared.insert(resource.name.name.clone(), (file, item));
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut world = World {
            package,
            service: inputs.service,
            roles,
            parents,
            subjects: Vec::new(),
        };
        world.collect(inputs, &declared);
        world
    }

    fn collect(&mut self, inputs: &Inputs<'a>, resources: &BTreeMap<String, (usize, &Item)>) {
        let service = inputs.service;
        let package = self.package;
        let mut subjects = Vec::new();
        if service.id.namespace() == package {
            subjects.push(Subject {
                entity: Entity::Service(service),
                name: service.id.name().to_owned(),
                display: service.id.name().to_owned(),
                aliases: Vec::new(),
                id: service.id.clone(),
                declared: None,
            });
        }
        for (name, (file, item)) in resources {
            subjects.push(Subject {
                entity: Entity::Resource {
                    documented: item.doc.is_some(),
                },
                name: name.clone(),
                display: name.clone(),
                aliases: Vec::new(),
                id: ShapeId::new(package, name),
                declared: Some((*file, item.span)),
            });
        }
        let mut io: BTreeMap<&ShapeId, (Side, &str)> = BTreeMap::new();
        for operation in &service.operations {
            if operation.id.namespace() != package {
                continue;
            }
            if let Some(input) = &operation.input {
                io.insert(input, (Side::Input, operation.name()));
            }
            if let Some(output) = &operation.output {
                io.insert(output, (Side::Output, operation.name()));
            }
            subjects.push(Subject {
                entity: Entity::Operation(operation),
                name: operation.name().to_owned(),
                display: operation.name().to_owned(),
                aliases: Vec::new(),
                id: operation.id.clone(),
                declared: None,
            });
        }
        let referenced = self.referenced(inputs);
        for (id, shape) in &service.shapes {
            if id.namespace() != package {
                continue;
            }
            let Some(class) = Class::of(shape) else {
                continue;
            };
            let shape_parts = parts(shape);
            subjects.push(Subject {
                entity: Entity::Shape {
                    shape: Some(shape),
                    class,
                    documented: shape_parts.docs.is_some_and(|d| !d.trim().is_empty()),
                    referenced: true,
                },
                name: id.name().to_owned(),
                display: id.name().to_owned(),
                aliases: Vec::new(),
                id: id.clone(),
                declared: None,
            });
            let side = io.get(id).copied();
            let members: Vec<(&str, &Member)> = match shape {
                Shape::Structure(s) | Shape::Union(s) => {
                    s.members.iter().map(|m| (m.name.as_str(), m)).collect()
                }
                Shape::List(list) => vec![("member", &list.member)],
                Shape::Map(map) => vec![("key", &map.key), ("value", &map.value)],
                _ => Vec::new(),
            };
            for (name, member) in members {
                let member_id = id.with_member(name);
                let mut aliases = vec![id.name().to_owned()];
                aliases.extend(side.map(|(_, operation)| operation.to_owned()));
                subjects.push(Subject {
                    entity: Entity::Member {
                        member,
                        side: side.map(|(side, _)| side),
                        template: inputs.template_members.contains(&member_id),
                    },
                    name: name.to_owned(),
                    display: format!("{}.{name}", id.name()),
                    aliases,
                    id: member_id,
                    declared: None,
                });
            }
        }
        // Shapes nothing reaches were never lowered: they are subjects for
        // `referenced`, `documented` and their name and kind alone.
        for (file, parsed) in inputs.parsed.iter().enumerate() {
            if inputs.namespaces.get(file).map(String::as_str) != Some(package) {
                continue;
            }
            for item in &parsed.file.items {
                let Some((name, class)) = declared_shape(&item.decl) else {
                    continue;
                };
                let id = ShapeId::new(package, &name);
                if service.shapes.contains_key(&id) {
                    continue;
                }
                subjects.push(Subject {
                    entity: Entity::Shape {
                        shape: None,
                        class,
                        documented: item.doc.is_some(),
                        referenced: referenced.contains(&name),
                    },
                    display: name.clone(),
                    name,
                    aliases: Vec::new(),
                    id,
                    declared: Some((file, item.span)),
                });
            }
        }
        self.subjects = subjects;
    }

    /// The names of the package's declared shapes something reaches: what
    /// operations reach (already lowered), what resources and metadata name,
    /// and whatever those reach in turn, including through `include`.
    fn referenced(&self, inputs: &Inputs<'a>) -> BTreeSet<String> {
        let package = self.package;
        let mut members: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (file, parsed) in inputs.parsed.iter().enumerate() {
            if inputs.namespaces.get(file).map(String::as_str) != Some(package) {
                continue;
            }
            for item in &parsed.file.items {
                if let Some((name, _)) = declared_shape(&item.decl) {
                    members.insert(name, names_in(&item.decl));
                }
            }
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut pending: Vec<String> = inputs
            .service
            .shapes
            .keys()
            .chain(inputs.named_types)
            .filter(|id| id.namespace() == package)
            .map(|id| id.name().to_owned())
            .collect();
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            pending.extend(members.get(&name).into_iter().flatten().cloned());
        }
        seen
    }

    /// The operation's role word, if it attaches to a resource.
    pub(super) fn role(&self, operation: &str) -> Option<&str> {
        self.roles.get(operation).map(String::as_str)
    }

    /// Whether `resource` is `ancestor` or below it.
    pub(super) fn is_within(&self, resource: &str, ancestor: &str) -> bool {
        let mut current = Some(resource);
        let mut steps = 0;
        while let Some(name) = current {
            if name == ancestor {
                return true;
            }
            steps += 1;
            if steps > 64 {
                return false;
            }
            current = self.parents.get(name).and_then(|p| p.as_deref());
        }
        false
    }

    pub(super) fn has_resource(&self, name: &str) -> bool {
        self.parents.contains_key(name)
    }
}

/// A declared type: its name and what it is.
fn declared_shape(decl: &Decl) -> Option<(String, Class)> {
    Some(match decl {
        Decl::Structure(s) => (s.name.name.clone(), Class::Structure),
        Decl::Union(u) => (u.name.name.clone(), Class::Union),
        Decl::Error(e) => (e.name.name.clone(), Class::Error),
        Decl::Enum(e) if e.base.is_some() => (e.name.name.clone(), Class::IntEnum),
        Decl::Enum(e) => (e.name.name.clone(), Class::StringEnum),
        Decl::List(l) => (l.name.name.clone(), Class::List),
        Decl::Map(m) => (m.name.name.clone(), Class::Map),
        Decl::Scalar(s) => (
            s.name.name.clone(),
            Class::Simple(scalar_kind(&s.kind.name)?),
        ),
        _ => return None,
    })
}

fn scalar_kind(word: &str) -> Option<SimpleKind> {
    Some(match word {
        "string" => SimpleKind::String,
        "boolean" => SimpleKind::Boolean,
        "byte" => SimpleKind::Byte,
        "short" => SimpleKind::Short,
        "integer" => SimpleKind::Integer,
        "long" => SimpleKind::Long,
        "float" => SimpleKind::Float,
        "double" => SimpleKind::Double,
        "bigInteger" => SimpleKind::BigInteger,
        "bigDecimal" => SimpleKind::BigDecimal,
        "timestamp" => SimpleKind::Timestamp,
        "blob" => SimpleKind::Blob,
        "document" => SimpleKind::Document,
        _ => return None,
    })
}

/// The type names a declaration's members, items or values name.
fn names_in(decl: &Decl) -> Vec<String> {
    let mut out = Vec::new();
    let members = |members: &[crate::ast::Member], out: &mut Vec<String>| {
        for member in members {
            match &member.kind {
                MemberKind::Field { ty: Some(ty), .. } => out.push(ty.name.name.clone()),
                MemberKind::Include(name) => out.push(name.name.clone()),
                _ => {}
            }
        }
    };
    match decl {
        Decl::Structure(s) => members(&s.members, &mut out),
        Decl::Union(u) => members(&u.members, &mut out),
        Decl::Error(e) => {
            for entry in &e.entries {
                if let crate::ast::ErrorEntry::Member(m) = entry {
                    members(std::slice::from_ref(m), &mut out);
                }
            }
        }
        Decl::List(l) => out.push(l.member.name.clone()),
        Decl::Map(m) => {
            out.push(m.key.name.clone());
            out.push(m.value.name.clone());
        }
        _ => {}
    }
    out
}
