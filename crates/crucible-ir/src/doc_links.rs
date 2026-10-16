//! `[Name]` cross-references in documentation Markdown (decision 80 in
//! docs/idl/decisions.md).
//!
//! A doc link is a CommonMark reference link that the comment itself does
//! not define, whose label is a declaration's name, optionally with one of
//! its members: `[CreateApiKey]`, `[City.name]`, the collapsed
//! `[CreateApiKey][]` and the full `[the key call][CreateApiKey]`. A
//! `[Name]: https://..` definition in the same comment makes `[Name]` an
//! ordinary link, as CommonMark says. The front end resolves each label to
//! an id once, into [`crate::ir::Service::doc_links`]; every renderer finds
//! the same links with [`links`], since one parser, used one way, decides
//! what is a link.

use std::ops::Range;

use pulldown_cmark::{BrokenLink, LinkType, Options, Parser};

use crate::ShapeId;
use crate::ir::{Member, Operation, Resource, Service, Shape};

/// One doc link in a documentation string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocLink {
    /// The label as written: `CreateApiKey` or `City.name`.
    pub label: String,
    /// The bytes of the whole link: `[City.name]`, `[City.name][]` or
    /// `[the city][City]`.
    pub span: Range<usize>,
    /// The bytes of the label inside [`DocLink::span`].
    pub label_span: Range<usize>,
    /// Whether the link has text of its own (`[the city][City]`), which a
    /// renderer shows instead of the label.
    pub full: bool,
}

/// Whether `text` is a doc link label: a capitalised identifier, then
/// optionally `.` and a member or enum value name. Anything else in
/// brackets (`[1]`, `[optional]`, `[a, b]`) is prose.
pub fn is_label(text: &str) -> bool {
    let (name, member) = match text.split_once('.') {
        Some((name, member)) => (name, Some(member)),
        None => (text, None),
    };
    let identifier = |s: &str| {
        s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    name.starts_with(|c: char| c.is_ascii_uppercase())
        && identifier(name)
        && member.is_none_or(identifier)
}

/// Every doc link in `markdown`, in source order: the reference links the
/// text does not define whose label [`is_label`]. Links in code spans and
/// blocks, inline links and images are not doc links.
pub fn links(markdown: &str) -> Vec<DocLink> {
    let mut found = Vec::new();
    let callback = |link: BrokenLink<'_>| {
        if let Some(found_link) = classify(markdown, &link) {
            found.push(found_link);
        }
        None
    };
    Parser::new_with_broken_link_callback(markdown, Options::empty(), Some(callback))
        .for_each(drop);
    found.sort_by_key(|link| (link.span.start, link.span.end));
    found.dedup();
    found
}

/// What a link the parser could not resolve is, if it is a doc link: the
/// renderers' callbacks call it to decide which links to take over.
pub fn classify(markdown: &str, link: &BrokenLink<'_>) -> Option<DocLink> {
    let label = link.reference.as_ref();
    if !is_label(label) {
        return None;
    }
    let mut span = link.span.clone();
    // The parser's span of a collapsed `[Label][]` stops before the `[]`.
    if matches!(link.link_type, LinkType::Collapsed) && markdown[span.end..].starts_with("[]") {
        span.end += 2;
    }
    let source = markdown.get(span.clone())?;
    // An image reference (`![Name]`) is not a link to anything.
    if source.starts_with('!') || markdown[..span.start].ends_with('!') {
        return None;
    }
    let full = matches!(link.link_type, LinkType::Reference);
    let offset = if full {
        // `[text][Label]`: the label is in the last brackets.
        source.rfind(label)?
    } else {
        // `[Label]` or `[Label][]`.
        source.find(label)?
    };
    let start = span.start + offset;
    Some(DocLink {
        label: label.to_owned(),
        span,
        label_span: start..start + label.len(),
        full,
    })
}

/// What a doc link names in a service, as [`Service::doc_link`] finds it.
#[derive(Debug, Clone, Copy)]
pub enum DocTarget<'a> {
    /// The service itself.
    Service,
    Operation(&'a Operation),
    Resource(&'a Resource),
    /// A shape, by id: a structure, union, error, enum or other type.
    Shape(&'a ShapeId, &'a Shape),
    /// A member of a structure, union or error; for `[Operation.member]`,
    /// of the operation's input or output structure.
    Member {
        shape: &'a ShapeId,
        owner: &'a Shape,
        member: &'a Member,
    },
    /// A value of an enum or int enum, by its name.
    EnumValue {
        shape: &'a ShapeId,
        owner: &'a Shape,
        value: &'a str,
    },
}

impl DocTarget<'_> {
    /// The id the link resolves to: the shape, operation, resource or
    /// service, with the member or enum value for those.
    pub fn id(&self, service: &Service) -> ShapeId {
        match self {
            Self::Service => service.id.clone(),
            Self::Operation(op) => op.id.clone(),
            Self::Resource(resource) => resource.id.clone(),
            Self::Shape(id, _) => (*id).clone(),
            Self::Member { shape, member, .. } => shape.with_member(&member.name),
            Self::EnumValue { shape, value, .. } => shape.with_member(value),
        }
    }
}

impl Service {
    /// What the doc link `label` names in this service, or `None` when it
    /// names nothing here: a prelude type, a declaration the service does
    /// not reach, or one an audience projection removed. Renderers show a
    /// label without a target as plain text.
    pub fn doc_link(&self, label: &str) -> Option<DocTarget<'_>> {
        let id = self.doc_links.get(label)?;
        if *id == self.id {
            return Some(DocTarget::Service);
        }
        let root = id.root();
        if let Some(member) = id.member() {
            let (shape, owner) = self.shapes.get_key_value(&root)?;
            return match owner {
                Shape::Structure(s) | Shape::Union(s) => s
                    .members
                    .iter()
                    .find(|m| m.name == member)
                    .map(|member| DocTarget::Member {
                        shape,
                        owner,
                        member,
                    }),
                Shape::Enum(e) => {
                    e.values
                        .iter()
                        .find(|v| v.name == member)
                        .map(|v| DocTarget::EnumValue {
                            shape,
                            owner,
                            value: &v.name,
                        })
                }
                Shape::IntEnum(e) => {
                    e.values
                        .iter()
                        .find(|v| v.name == member)
                        .map(|v| DocTarget::EnumValue {
                            shape,
                            owner,
                            value: &v.name,
                        })
                }
                _ => None,
            };
        }
        if let Some(op) = self.operations.iter().find(|op| op.id == root) {
            return Some(DocTarget::Operation(op));
        }
        if let Some(resource) = self.resources.get(&root) {
            return Some(DocTarget::Resource(resource));
        }
        let (shape, owner) = self.shapes.get_key_value(&root)?;
        (!root.is_prelude()).then_some(DocTarget::Shape(shape, owner))
    }

    /// Drops the doc links whose target this service no longer has, as
    /// after an audience projection: they render as plain text, and the
    /// projected IR names no id it removed.
    pub fn retain_doc_links(&mut self) {
        let labels: Vec<String> = self
            .doc_links
            .keys()
            .filter(|label| self.doc_link(label).is_none())
            .cloned()
            .collect();
        for label in labels {
            self.doc_links.remove(&label);
        }
    }
}

/// The name and, for `Name.member`, the member a label names.
pub fn split(label: &str) -> (&str, Option<&str>) {
    match label.split_once('.') {
        Some((name, member)) => (name, Some(member)),
        None => (label, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(text: &str) -> Vec<String> {
        links(text).into_iter().map(|link| link.label).collect()
    }

    #[test]
    fn finds_shortcut_collapsed_and_full_references() {
        let text = "See [CreateApiKey], [City.name][] and [the city][GetCity].";
        let found = links(text);
        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(&text[found[0].span.clone()], "[CreateApiKey]");
        assert_eq!(&text[found[0].label_span.clone()], "CreateApiKey");
        assert!(!found[0].full);
        assert_eq!(&text[found[1].span.clone()], "[City.name][]");
        assert_eq!(&text[found[1].label_span.clone()], "City.name");
        assert_eq!(&text[found[2].span.clone()], "[the city][GetCity]");
        assert_eq!(&text[found[2].label_span.clone()], "GetCity");
        assert!(found[2].full);
    }

    #[test]
    fn code_definitions_inline_links_images_and_prose_are_not_doc_links() {
        assert!(labels("`[Code]` and\n\n```\n[Fenced]\n```\n\n    [Indented]").is_empty());
        assert!(labels("[Defined] here.\n\n[Defined]: https://example.com").is_empty());
        assert!(labels("[Inline](https://example.com) ![Image]").is_empty());
        assert!(labels("[1], [optional], [a, b], [lower.case], [A.b.c], [A-b]").is_empty());
        assert!(labels(r"\[Escaped]").is_empty());
    }

    #[test]
    fn a_link_across_lines_keeps_its_label_span() {
        let text = "first line\nthen [the\ncity][GetCity] ends";
        let found = links(text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(&text[found[0].label_span.clone()], "GetCity");
    }

    #[test]
    fn labels_are_names_with_an_optional_member() {
        assert!(is_label("City"));
        assert!(is_label("City.name"));
        assert!(is_label("Status.ACTIVE"));
        assert!(is_label("City._x"));
        assert!(!is_label("city"));
        assert!(!is_label("City."));
        assert!(!is_label("City.a.b"));
        assert!(!is_label("City name"));
    }
}
