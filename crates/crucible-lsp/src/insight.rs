//! What the editor shows beside the code rather than in it: inlay hints for
//! what the source leaves implicit, links in comments and strings, the type a
//! member has, and the parameters of the annotation or clause being written.

use std::collections::HashMap;

use crucible_lang::ast::{
    Body, Decl, ErrorEntry, Ident, Member, MemberKind, MetaType, OpEntry, Resource,
};
use crucible_lang::{Parsed, Token, TokenKind};

/// A hint drawn at a byte offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub offset: u32,
    pub label: String,
    pub kind: HintKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintKind {
    /// The type an elided `$member` takes from its resource.
    Type,
    /// The wire value of an enum value written without one.
    Value,
}

/// Every resource declared in `files`, by name.
fn resources<'a>(files: &[&'a Parsed]) -> HashMap<&'a str, &'a Resource> {
    let mut out = HashMap::new();
    for parsed in files {
        for item in &parsed.file.items {
            if let Decl::Resource(resource) = &item.decl {
                out.entry(resource.name.name.as_str()).or_insert(resource);
            }
        }
    }
    out
}

/// The type of a resource's identifier or property, looking through its
/// parents as lowering does.
fn entry_type<'a>(
    resources: &HashMap<&str, &'a Resource>,
    resource: &str,
    name: &str,
) -> Option<&'a Ident> {
    let mut current = Some(resource);
    let mut seen = Vec::new();
    while let Some(resource) = current {
        if seen.contains(&resource) {
            return None;
        }
        seen.push(resource);
        let declaration = resources.get(resource)?;
        if let Some(entry) = declaration
            .entries
            .iter()
            .find(|entry| entry.name.name == name)
        {
            return Some(&entry.ty.name);
        }
        current = declaration
            .parent
            .as_ref()
            .map(|parent| parent.name.as_str());
    }
    None
}

/// Visits every member list in `parsed` with the resource its elided
/// members draw from, if any.
fn member_lists<'a>(parsed: &'a Parsed, mut visit: impl FnMut(&'a [Member], Option<&'a str>)) {
    for item in &parsed.file.items {
        match &item.decl {
            Decl::Structure(structure) => visit(
                &structure.members,
                structure.resource.as_ref().map(|r| r.name.as_str()),
            ),
            Decl::Union(union) => visit(&union.members, None),
            Decl::Error(error) => {
                for entry in &error.entries {
                    if let ErrorEntry::Member(member) = entry {
                        visit(std::slice::from_ref(member), None);
                    }
                }
            }
            Decl::Operation(operation) => {
                let resource = operation
                    .attachment
                    .as_ref()
                    .map(|attachment| attachment.resource.name.as_str());
                for entry in &operation.entries {
                    if let OpEntry::Body(Body { members, .. }) = entry {
                        visit(members, resource);
                    }
                }
            }
            Decl::Template(template) => {
                for entry in &template.entries {
                    if let OpEntry::Body(Body { members, .. }) = entry {
                        visit(members, None);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Hints between `start` and `end`: the type of each `$member` and the
/// value of each string enum value written without one. `package` is every
/// file of the package, so a resource declared elsewhere is found.
pub fn inlay_hints(parsed: &Parsed, package: &[Parsed], start: u32, end: u32) -> Vec<Hint> {
    let mut files: Vec<&Parsed> = vec![parsed];
    files.extend(package);
    let resources = resources(&files);
    let mut out = Vec::new();
    member_lists(parsed, |members, resource| {
        let Some(resource) = resource else { return };
        for member in members {
            if let MemberKind::Elided { name, alias, .. } = &member.kind
                && let Some(ty) = entry_type(&resources, resource, &name.name)
            {
                let at = alias.as_ref().unwrap_or(name).span.end;
                out.push(Hint {
                    offset: at,
                    label: format!(": {}", ty.name),
                    kind: HintKind::Type,
                });
            }
        }
    });
    for item in &parsed.file.items {
        if let Decl::Enum(enumeration) = &item.decl {
            // An integer enum's values are numbers the source must give.
            if enumeration
                .base
                .as_ref()
                .is_some_and(|base| base.name != "string")
            {
                continue;
            }
            for value in &enumeration.values {
                if value.value.is_none() && !value.name.is_missing() {
                    out.push(Hint {
                        offset: value.name.span.end,
                        label: format!("= \"{}\"", value.name.name),
                        kind: HintKind::Value,
                    });
                }
            }
        }
    }
    out.retain(|hint| start <= hint.offset && hint.offset <= end);
    out.sort_by_key(|hint| hint.offset);
    out
}

/// The name of the type the member or resource entry named at `offset`
/// has: what "go to type" opens from a member's name.
pub fn type_name_at(parsed: &Parsed, package: &[Parsed], offset: u32) -> Option<String> {
    let covers = |ident: &Ident| ident.span.start <= offset && offset <= ident.span.end;
    for item in &parsed.file.items {
        if let Decl::Resource(resource) = &item.decl
            && let Some(entry) = resource.entries.iter().find(|entry| covers(&entry.name))
        {
            return Some(entry.ty.name.name.clone());
        }
    }
    let mut files: Vec<&Parsed> = vec![parsed];
    files.extend(package);
    let resources = resources(&files);
    let mut found = None;
    member_lists(parsed, |members, resource| {
        for member in members {
            match &member.kind {
                MemberKind::Field {
                    name, ty: Some(ty), ..
                } if covers(name) => found = Some(ty.name.name.clone()),
                MemberKind::Elided { name, alias, .. }
                    if covers(name) || alias.as_ref().is_some_and(covers) =>
                {
                    found = resource
                        .and_then(|resource| entry_type(&resources, resource, &name.name))
                        .map(|ty| ty.name.clone());
                }
                _ => {}
            }
        }
    });
    found
}

/// `http://` and `https://` links in comments and strings, as byte ranges
/// and targets.
pub fn links(parsed: &Parsed, text: &str) -> Vec<(u32, u32, String)> {
    let mut out = Vec::new();
    for token in &parsed.tokens {
        if !matches!(
            token.kind,
            TokenKind::LineComment | TokenKind::DocComment | TokenKind::String
        ) {
            continue;
        }
        let body = token.span.text(text);
        let mut from = 0;
        while let Some(found) = ["https://", "http://"]
            .iter()
            .filter_map(|scheme| body[from..].find(scheme))
            .min()
        {
            let start = from + found;
            let rest = &body[start..];
            let mut end = start
                + rest
                    .find(|c: char| c.is_whitespace() || "\"'<>`()[]{}".contains(c))
                    .unwrap_or(rest.len());
            // Sentence punctuation after a link is not part of it.
            while body[start..end].ends_with(['.', ',', ';', ':', '!', '?']) {
                end -= 1;
            }
            let url = &body[start..end];
            if url.len() > "https://".len() {
                let base = token.span.start;
                out.push((base + start as u32, base + end as u32, url.to_owned()));
            }
            from = end.max(start + 1);
        }
    }
    out
}

/// A signature to show while arguments are written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub label: String,
    pub documentation: String,
    pub parameters: Vec<String>,
    pub active: u32,
}

/// The built-in annotations and clauses that take arguments: name, the
/// parameters, what it does. Named parameters are written `name:`; the
/// compiler's checks in `lower.rs` are the source of these.
const SIGNATURES: &[(&str, bool, &[&str], &str)] = &[
    (
        "since",
        true,
        &["version: string"],
        "The version that added it.",
    ),
    (
        "deprecated",
        true,
        &["message: string", "since: string"],
        "Deprecated, with why and since when.",
    ),
    (
        "externalDocs",
        true,
        &["\"Name\" = \"https://…\""],
        "Links to documentation, as name and URL pairs.",
    ),
    (
        "query",
        true,
        &["name?: string"],
        "Bound to a query parameter, by default named after the member.",
    ),
    (
        "header",
        true,
        &["name: string"],
        "Bound to an HTTP header.",
    ),
    (
        "mutability",
        true,
        &["full | create | createAndRead | read | write"],
        "When a resource property can be written and read.",
    ),
    (
        "paginated",
        false,
        &[
            "inputToken: member",
            "outputToken: member",
            "items: member",
            "pageSize: member",
        ],
        "Pagination: the members that carry the token, the items and the page size.",
    ),
];

/// The signature of the annotation or clause call whose arguments surround
/// `offset`, with the parameter being written.
pub fn signature_at(
    parsed: &Parsed,
    package: &[Parsed],
    text: &str,
    offset: usize,
) -> Option<Signature> {
    let tokens: Vec<&Token> = parsed
        .tokens
        .iter()
        .filter(|token| !token.kind.is_trivia() && (token.span.end as usize) <= offset)
        .collect();
    // The innermost `(` still open at the caret.
    let mut open: Vec<usize> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => open.push(index),
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                open.pop();
            }
            _ => {}
        }
    }
    let paren = *open
        .iter()
        .rev()
        .find(|index| tokens[**index].kind == TokenKind::LParen)?;
    let name = tokens.get(paren.checked_sub(1)?)?;
    if name.kind != TokenKind::Ident {
        return None;
    }
    let name_text = name.span.text(text);
    let annotation = paren >= 2 && tokens[paren - 2].kind == TokenKind::At;

    // Which argument the caret is in: commas at this level, or a named key.
    let mut depth = 0;
    let mut position = 0;
    let mut key: Option<&str> = None;
    for (index, token) in tokens.iter().enumerate().skip(paren + 1) {
        match token.kind {
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => depth -= 1,
            TokenKind::Comma if depth == 0 => {
                position += 1;
                key = None;
            }
            TokenKind::Ident
                if depth == 0
                    && tokens
                        .get(index + 1)
                        .is_some_and(|next| next.kind == TokenKind::Colon) =>
            {
                key = Some(token.span.text(text));
            }
            _ => {}
        }
    }

    if let Some((label, params, documentation)) = SIGNATURES
        .iter()
        .find(|(n, is_annotation, _, _)| *n == name_text && *is_annotation == annotation)
        .map(|(n, _, params, doc)| {
            let prefix = if annotation { "@" } else { "" };
            (
                format!("{prefix}{n}({})", params.join(", ")),
                params.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>(),
                (*doc).to_owned(),
            )
        })
    {
        let active = key
            .and_then(|key| {
                params
                    .iter()
                    .position(|param| param.split([':', '?']).next() == Some(key))
            })
            .unwrap_or(position.min(params.len().saturating_sub(1)));
        return Some(Signature {
            label,
            documentation,
            parameters: params,
            active: active as u32,
        });
    }
    if !annotation {
        return None;
    }
    // A metadata annotation takes one value of the metadata's type.
    let mut files: Vec<&Parsed> = vec![parsed];
    files.extend(package);
    for file in files {
        for item in &file.file.items {
            if let Decl::Metadata(metadata) = &item.decl
                && metadata.name.name == name_text
            {
                let parameter = match &metadata.ty {
                    Some(MetaType::Type(ty)) => format!("value: {}", ty.name.name),
                    Some(MetaType::Enum(values)) => values
                        .iter()
                        .map(|value| value.name.name.as_str())
                        .collect::<Vec<_>>()
                        .join(" | "),
                    None => return None,
                };
                return Some(Signature {
                    label: format!("@{name_text}({parameter})"),
                    documentation: item
                        .doc
                        .as_ref()
                        .map(|doc| doc.text.trim().to_owned())
                        .unwrap_or_default(),
                    parameters: vec![parameter],
                    active: 0,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
resource City {
    id name: CityName
    property size: integer
}
resource District in City {
    id district: string
}

operation GetDistrict reads District {
    input { $name, $district as id }
}

structure CityView for City {
    $size?
    label: string
}

enum Kind { BIG, SMALL = "small" }
enum Level: integer { LOW = 1 }
"#;

    fn label_at(hints: &[Hint], source: &str, after: &str) -> Option<String> {
        let at = (source.find(after).unwrap() + after.len()) as u32;
        hints
            .iter()
            .find(|hint| hint.offset == at)
            .map(|hint| hint.label.clone())
    }

    #[test]
    fn hints_show_elided_types_and_implicit_enum_values() {
        let parsed = crucible_lang::parse(SOURCE);
        let hints = inlay_hints(&parsed, &[], 0, SOURCE.len() as u32);
        assert_eq!(
            label_at(&hints, SOURCE, "{ $name").as_deref(),
            Some(": CityName")
        );
        assert_eq!(
            label_at(&hints, SOURCE, "as id").as_deref(),
            Some(": string")
        );
        assert_eq!(
            label_at(&hints, SOURCE, "$size").as_deref(),
            Some(": integer")
        );
        assert_eq!(
            label_at(&hints, SOURCE, "{ BIG").as_deref(),
            Some("= \"BIG\"")
        );
        assert_eq!(label_at(&hints, SOURCE, "SMALL"), None);
        assert_eq!(label_at(&hints, SOURCE, "LOW"), None);
        assert_eq!(hints.len(), 4);
        // Only the requested range.
        let start = SOURCE.find("enum Kind").unwrap() as u32;
        assert_eq!(
            inlay_hints(&parsed, &[], start, SOURCE.len() as u32).len(),
            1
        );
    }

    #[test]
    fn a_resource_in_another_file_is_found() {
        let resources = crucible_lang::parse("resource City { id name: CityName }");
        let source = "operation GetCity reads City { input { $name } }";
        let parsed = crucible_lang::parse(source);
        let hints = inlay_hints(&parsed, &[resources], 0, source.len() as u32);
        assert_eq!(hints[0].label, ": CityName");
    }

    #[test]
    fn a_members_type_is_found_from_its_name() {
        let parsed = crucible_lang::parse(SOURCE);
        let at = |word: &str| {
            let offset = SOURCE.find(word).unwrap() as u32 + 1;
            type_name_at(&parsed, &[], offset)
        };
        assert_eq!(at("name: CityName").as_deref(), Some("CityName"));
        assert_eq!(at("label: string").as_deref(), Some("string"));
        assert_eq!(at("district as").as_deref(), Some("string"));
        assert_eq!(at("CityView"), None);
    }

    #[test]
    fn links_are_found_in_comments_and_strings() {
        let source = "/// See https://example.com/docs.\n// (http://a.io/x) and nothing://else\n@externalDocs(\"Guide\" = \"https://g.dev/p?q=1\")\nstructure A {}\n";
        let parsed = crucible_lang::parse(source);
        let found: Vec<&str> = links(&parsed, source)
            .iter()
            .map(|(start, end, url)| {
                assert_eq!(&source[*start as usize..*end as usize], url);
                &source[*start as usize..*end as usize]
            })
            .collect();
        assert_eq!(
            found,
            [
                "https://example.com/docs",
                "http://a.io/x",
                "https://g.dev/p?q=1"
            ]
        );
    }

    fn signature(source: &str) -> Option<Signature> {
        let offset = source.find('|').unwrap();
        let text = source.replace('|', "");
        let parsed = crucible_lang::parse(&text);
        signature_at(&parsed, &[], &text, offset)
    }

    #[test]
    fn signatures_follow_the_argument_being_written() {
        let since = signature("@since(|").unwrap();
        assert_eq!(since.label, "@since(version: string)");
        assert_eq!(since.active, 0);
        let deprecated = signature("@deprecated(message: \"x\", |").unwrap();
        assert_eq!(deprecated.active, 1);
        let by_key = signature("@deprecated(since: |").unwrap();
        assert_eq!(by_key.active, 1);
        let paginated = signature("operation A { paginated(items: x, pageSize: |) }").unwrap();
        assert_eq!(paginated.label.split('(').next(), Some("paginated"));
        assert_eq!(paginated.active, 3);
        assert_eq!(signature("structure A { a: string }|"), None);
        assert_eq!(signature("@unknown(|"), None);
        // `paginated` is a clause, not an annotation.
        assert_eq!(signature("@paginated(|"), None);
    }

    #[test]
    fn metadata_annotations_take_their_declared_type() {
        let signature = signature(
            "/// Who owns it.\nmetadata owner on operation: string\nmetadata tier on operation: enum { GOLD SILVER }\n@owner(|",
        )
        .unwrap();
        assert_eq!(signature.label, "@owner(value: string)");
        assert_eq!(signature.documentation, "Who owns it.");
    }
}
