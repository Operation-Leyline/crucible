//! Upgrade guides (docs/api-versions-clients.md, 5d; decision 57a): what a
//! client moving from one release to a newer one meets, in the model's
//! terms. A guide reads only the description's `versions` block and its
//! elements' `history`, which `crucible_ir::versions` settles once for the
//! reference, `changes.json` and these guides alike, so a guide cannot say
//! what the release pages do not.
//!
//! First the sunsets of the releases from the older to the newer. Then
//! each migration between them is a section, oldest first: its
//! documentation, each change as its release page renders it (what changed,
//! the step's documentation, what a client of the release before it sees
//! instead), and the operations it retired. Last, what was added after the
//! older release up to the newer, which reaches older releases too unless
//! an entry says otherwise (decision 51).

use std::collections::BTreeSet;

use crucible_codegen_common::describe::Description;
use serde_json::Value;

use super::{
    change_section, code, docs, element, heading, json_doc, label, list, literal, release_code,
    release_file, release_link, string,
};

/// A guide's page, relative to `src/`: `upgrade/<from>_to_<to>.md`, each
/// name escaped as its release page's file is. [`release_file`] writes `_`
/// only before two hex digits, so `_to_` cannot occur inside a name, and
/// no two pairs share a file, where case folds too (release names that
/// fold to one file are refused already).
pub(in crate::markdown) fn path(from: &str, to: &str) -> String {
    format!("upgrade/{}_to_{}.md", release_file(from), release_file(to))
}

/// The supported releases, oldest first, current last.
fn releases(d: &Description) -> &[Value] {
    list(&d.versions["releases"])
}

/// The guides a reference carries, by release index: from each release to
/// the next one and to the current one, oldest first, then by target.
pub(in crate::markdown) fn pairs(d: &Description) -> Vec<(usize, usize)> {
    let count = releases(d).len();
    let mut out = Vec::new();
    for from in 0..count.saturating_sub(1) {
        let targets: BTreeSet<usize> = [from + 1, count - 1].into_iter().collect();
        out.extend(targets.into_iter().map(|to| (from, to)));
    }
    out
}

/// The releases a guide goes between, by index: `from` and `to` (the
/// current release when `None`) must be supported, and `from` older. The
/// message names the supported releases.
pub(crate) fn select(
    d: &Description,
    from: &str,
    to: Option<&str>,
) -> Result<(usize, usize), String> {
    let names: Vec<&str> = releases(d).iter().map(|r| string(&r["name"])).collect();
    if names.is_empty() {
        return Err(format!(
            "upgrade guide: {} names no release (a service without an `api_version` has no releases to upgrade between)",
            string(&d.service["id"])
        ));
    }
    let supported = || {
        names
            .iter()
            .map(|name| format!("{name:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let find = |name: &str| {
        names.iter().position(|n| *n == name).ok_or_else(|| {
            format!(
                "upgrade guide: {name:?} is not a supported release; the supported releases are {}, oldest first",
                supported()
            )
        })
    };
    let older = find(from)?;
    let newer = match to {
        Some(to) => find(to)?,
        None => names.len() - 1,
    };
    if older >= newer {
        return Err(format!(
            "upgrade guide: {from:?} is not older than {:?}: a guide goes from an older release to a newer one, and the supported releases are {}, oldest first",
            names[newer],
            supported()
        ));
    }
    Ok((older, newer))
}

/// The guide from release `from` to release `to`, by index, as a page in
/// `src/upgrade/`: its links are relative to that directory, and its
/// documentation's doc links are left for the reference to resolve.
pub(in crate::markdown) fn page(d: &Description, from: usize, to: usize) -> String {
    let releases = releases(d);
    let old = string(&releases[from]["name"]);
    let new = string(&releases[to]["name"]);
    let mut out = format!("# Upgrading from {} to {}\n\n", heading(old), heading(new));
    let title = string(&d.service["title"]);
    let current = if releases[to]["current"] == true {
        ", the current release"
    } else {
        ""
    };
    out.push_str(&format!(
        "Moving a client of the {} API from release {} to release {}{current}. A client names its release in the {} header, and a generated client sends the release it was generated for, so a client moves by being generated, or its SDK installed, for {}. Each section below is one migration, oldest first: what it changed, and what a client of the release before it sees instead.\n\n",
        literal(title),
        release_code(old),
        release_code(new),
        code(string(&d.versions["carrier"]["name"])),
        release_code(new),
    ));
    // The releases a client passes through, and the one it arrives at.
    let sunsets: Vec<&Value> = releases[from..=to]
        .iter()
        .filter(|release| release["sunset"].is_string())
        .collect();
    if !sunsets.is_empty() {
        out.push_str("## Sunset\n\n");
        for release in sunsets {
            out.push_str(&format!(
                "Release {} is supported until {}; after that the service may refuse its requests.\n\n",
                release_code(string(&release["name"])),
                release_code(string(&release["sunset"]))
            ));
            if !string(&release["sunsetDocumentation"]["html"]).is_empty() {
                out.push_str(&docs(&json_doc(&release["sunsetDocumentation"])));
            }
        }
    }
    for index in from + 1..=to {
        out.push_str(&migration(d, &releases[index - 1], &releases[index]));
    }
    out.push_str(&additions(d, from, to));
    out
}

/// One migration's section: the release it makes, its documentation, its
/// changes and the operations the release before it retired.
fn migration(d: &Description, before: &Value, release: &Value) -> String {
    let name = string(&release["name"]);
    let previous = string(&before["name"]);
    let mut out = format!(
        "## Release {}\n\nFollows {}.\n\n",
        heading(name),
        release_link(previous, "../")
    );
    if !string(&release["documentation"]["html"]).is_empty() {
        out.push_str(&docs(release));
    }
    if list(&release["changes"]).is_empty() {
        out.push_str("No change a client of the release before it would notice.\n\n");
    }
    for change in list(&release["changes"]) {
        out.push_str(&change_section(d, change, previous));
    }
    // `retired` sits on the release that had them: the next one dropped
    // them, and no release serves them any more.
    if !list(&before["retired"]).is_empty() {
        let retired: Vec<String> = list(&before["retired"])
            .iter()
            .map(|id| code(&label(string(id))))
            .collect();
        out.push_str(&format!(
            "### Retired operations\n\n{} dropped them, and no release serves them any more: {}.\n\n",
            release_code(name),
            retired.join(", ")
        ));
    }
    out
}

/// What kind of element an addition is, in the order a guide lists them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Operation,
    Type,
    Member,
    Value,
}

impl Kind {
    fn noun(self) -> &'static str {
        match self {
            Kind::Operation => "Operation",
            Kind::Type => "Data type",
            Kind::Member => "Member",
            Kind::Value => "Enum value",
        }
    }
}

/// "New since": every operation, data type, member and enum value added
/// after `from`, up to `to`. An addition reaches older releases too
/// (decision 51), unless its history says a release does not see it
/// (decision 74). Left out: the inputs and outputs of the operations added,
/// and the members and values of what is new itself, which come with it;
/// and what a change in the range introduces, which older clients see as
/// its `older` side says (an added value or error, a reshape's new member
/// or new structure), and its section lists.
fn additions(d: &Description, from: usize, to: usize) -> String {
    let releases = releases(d);
    let range: Vec<&str> = releases[from + 1..=to]
        .iter()
        .map(|r| string(&r["name"]))
        .collect();
    let seen_by: Vec<&str> = releases[from..=to]
        .iter()
        .map(|r| string(&r["name"]))
        .collect();
    let added = |item: &Value| {
        item["history"]["added"]
            .as_str()
            .and_then(|name| range.iter().position(|r| *r == name))
    };
    // What a change in the range introduces, by id.
    let mut changed = BTreeSet::new();
    for release in &releases[from + 1..=to] {
        for change in list(&release["changes"]) {
            match string(&change["kind"]) {
                "addEnumValue" => {
                    changed.insert(format!(
                        "{}${}",
                        string(&change["shape"]),
                        string(&change["value"])
                    ));
                }
                "addError" => {
                    changed.insert(string(&change["error"]).to_owned());
                }
                "reshape" => {
                    changed.insert(format!(
                        "{}${}",
                        string(&change["shape"]),
                        string(&change["into"])
                    ));
                    // A reshape into a structure of another id.
                    if change["shape"] != change["older"]["shape"] {
                        changed.insert(string(&change["shape"]).to_owned());
                    }
                }
                _ => {}
            }
        }
    }
    // The inputs and outputs of operations added in the range; an older
    // operation's new input or output is a data type of its own.
    let operation_shapes: BTreeSet<&str> = d
        .operations
        .values()
        .filter(|op| added(op).is_some())
        .flat_map(|op| [op["input"].as_str(), op["output"].as_str()])
        .flatten()
        .collect();
    let mut found: Vec<(usize, Kind, String, &Value)> = Vec::new();
    for (id, op) in &d.operations {
        if let Some(at) = added(op) {
            found.push((at, Kind::Operation, id.clone(), &op["history"]));
        }
    }
    for (id, shape) in &d.shapes {
        if crate::builtin(id) {
            continue;
        }
        if let Some(at) = added(shape) {
            if !operation_shapes.contains(id.as_str()) && !changed.contains(id) {
                found.push((at, Kind::Type, id.clone(), &shape["history"]));
            }
            continue;
        }
        for (key, kind) in [("members", Kind::Member), ("values", Kind::Value)] {
            for item in list(&shape[key]) {
                let element = format!("{id}${}", string(&item["name"]));
                if let Some(at) = added(item)
                    && !changed.contains(&element)
                {
                    found.push((at, kind, element, &item["history"]));
                }
            }
        }
    }
    found.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
    let old = string(&releases[from]["name"]);
    let new = string(&releases[to]["name"]);
    let mut out = format!("## New since {}\n\n", heading(old));
    if found.is_empty() {
        out.push_str(&format!(
            "Nothing was added after {}, up to {}.\n\n",
            release_code(old),
            release_code(new)
        ));
        return out;
    }
    out.push_str(&format!(
        "What was added after {}, up to {}. An older release is served as the current model sees it, so an addition reaches it too, except where an entry names a release that does not see it. One added in place of what an older release's own view had (under another name, or as internal API there) may still differ there: its release page says what changed.\n\n",
        release_code(old),
        release_code(new)
    ));
    for (at, kind, id, history) in found {
        let mut line = format!(
            "- {} {}: added in {}.",
            kind.noun(),
            element(d, &Value::String(id)),
            release_link(range[at], "../")
        );
        let missing: Vec<String> = list(&history["missing"])
            .iter()
            .map(string)
            .filter(|name| seen_by.contains(name))
            .map(|name| release_link(name, "../"))
            .collect();
        if !missing.is_empty() {
            line.push_str(&format!(
                " Clients of {} do not see it.",
                missing.join(", ")
            ));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push('\n');
    out
}

/// A guide standing on its own, outside a reference (`crucible
/// upgrade-guide`): every link to a reference page becomes its text, and
/// so does every doc link (decision 80), since there is no page for it,
/// and every other link but the documentation's web and mail addresses,
/// which are the author's and stay.
pub(crate) fn standalone(d: &Description, from: usize, to: usize) -> String {
    unlinked(&page(d, from, to))
}

/// Whether a link keeps its destination outside the reference: a web or
/// mail address. A relative path, a fragment and a root path name pages
/// or places a guide alone does not have.
fn kept(destination: &str) -> bool {
    let lower = destination.to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

/// `markdown` with only its web and mail links, Markdown and HTML, and
/// without its doc-link anchors, each link it drops keeping its text.
fn unlinked(markdown: &str) -> String {
    use pulldown_cmark::{Event, Parser, Tag};
    // Markdown links: the generator writes them inline, `[label](path)`.
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for (event, range) in Parser::new(markdown).into_offset_iter() {
        if let Event::Start(Tag::Link { dest_url, .. }) = event
            && !kept(&dest_url)
        {
            let source = &markdown[range.clone()];
            let written = format!("]({dest_url})");
            if source.starts_with('[')
                && source.ends_with(&written)
                && let Some(label) = source.get(1..source.len() - written.len())
            {
                edits.push((range, label.to_owned()));
            }
        }
    }
    edits.sort_by_key(|(range, _)| range.start);
    let mut text = String::with_capacity(markdown.len());
    let mut at = 0;
    for (range, label) in edits {
        if range.start < at {
            continue;
        }
        text.push_str(&markdown[at..range.start]);
        text.push_str(&label);
        at = range.end;
    }
    text.push_str(&markdown[at..]);
    // HTML anchors: the generator's and the documentation's, and doc
    // links, `<a data-crucible-ref="..">`. Model text is escaped in the
    // documentation's HTML, so `<a ` there is always markup.
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(start) = rest.find("<a ") {
        out.push_str(&rest[..start]);
        let tag = &rest[start..];
        let Some(close) = tag.find('>') else {
            out.push_str(tag);
            return out;
        };
        let open = &tag[..=close];
        let href = open
            .strip_prefix("<a href=\"")
            .and_then(|value| value.split('"').next());
        // An empty `href` is a link whose unsafe destination was removed.
        let drop =
            open.starts_with("<a data-crucible-ref=\"") || href.is_some_and(|href| !kept(href));
        if !drop {
            out.push_str(open);
            rest = &tag[close + 1..];
            continue;
        }
        // Links do not nest, so the next `</a>` closes this one.
        let body = &tag[close + 1..];
        let end = body.find("</a>").unwrap_or(body.len());
        out.push_str(&body[..end]);
        rest = body.get(end + 4..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}
