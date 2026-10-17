use crucible_codegen_common::describe::Description;
use crucible_codegen_common::documentation;
use crucible_ir::ir::Service;
use serde_json::Value;

use crate::File;

pub(crate) mod upgrade;

/// What a generated sample is, as the static reference says it too.
const GENERATED_NOTE: &str = "Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).";

/// What a request without the release header gets (docs/api-versions.md).
const VERSION_NOTE: &str = "A request without it is served at the release the service chooses for unversioned requests, which may be an older one.";

/// The release this reference documents: the service's `version`, which a
/// reference of an older release (an SDK line, decision 79) names while
/// `versions.current` names the current one. `None` for a service without
/// an `api_version`.
fn documented(d: &Description) -> Option<&str> {
    d.versions["current"].as_str()?;
    d.service["version"].as_str()
}

/// The documented and current releases, when this reference documents an
/// older release than the current one.
fn older_release(d: &Description) -> Option<(&str, &str)> {
    let current = d.versions["current"].as_str()?;
    let documented = documented(d)?;
    (documented != current).then_some((documented, current))
}

/// The sunset date of the release this reference documents, if it has
/// one: the date its migration declares, after which it may be refused.
fn documented_sunset(d: &Description) -> Option<&str> {
    let documented = documented(d)?;
    list(&d.versions["releases"])
        .iter()
        .find(|release| release["name"].as_str() == Some(documented))?["sunset"]
        .as_str()
}

/// Where the current release's reference is, from the root of this one:
/// beside it, in the directory [`crate::release_directory`] names, as
/// [`crate::generate_releases`] lays every release out.
pub(crate) fn current_reference(current: &str) -> String {
    format!("../{}/", crate::release_directory(current))
}

/// The notice at the top of every page of an older release's reference
/// (decision 79): which release it documents, until when that release is
/// served, and a link to the current release's reference, from a page at
/// `depth` directories below `src/`. Nothing for the current release.
fn release_banner(d: &Description, depth: usize) -> Option<String> {
    let (documented, current) = older_release(d)?;
    let sunset = documented_sunset(d).map_or_else(String::new, |date| {
        format!(", which is supported until {}", release_code(date))
    });
    // `src/` and mdBook's `book/` sit at the same depth below the root, so
    // one relative link reaches the current reference from either.
    let root = "../".repeat(depth + 1);
    Some(format!(
        "> **An older release.** This reference documents release {}{sunset}. The current release is {}: see [its reference]({root}{}index.html).\n\n",
        release_code(documented),
        release_code(current),
        current_reference(current)
    ))
}

/// The `Crucible-Version: <release>` line every request of a versioned
/// service sends at the release this reference documents, or nothing for a
/// service without an `api_version`.
fn version_header(d: &Description) -> Option<String> {
    let name = d.versions["carrier"]["name"].as_str()?;
    Some(format!("{name}: {}", documented(d)?))
}

/// The request header an authentication scheme puts on the wire, as the
/// request syntax shows it.
fn scheme_header(d: &Description, scheme: &str) -> Option<&'static str> {
    (d.service["authSchemes"][scheme]["kind"] == "bearer")
        .then_some("Authorization: Bearer {token}")
}

/// An authentication scheme, as a reader acts on it.
fn scheme_text(d: &Description, scheme: &str) -> String {
    match scheme_header(d, scheme) {
        Some(_) => format!(
            "Bearer token ({}): send {}.",
            code(scheme),
            code("Authorization: Bearer <token>")
        ),
        None => format!(
            "{}: {}",
            code(scheme),
            code(&d.service["authSchemes"][scheme].to_string())
        ),
    }
}

/// Text as a Markdown code span, whatever backticks it holds: a code span
/// reads nothing inside it as Markdown.
pub(crate) fn code_span(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let ticks = "`".repeat(longest + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{ticks}{pad}{text}{pad}{ticks}")
}

fn string(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}
fn list(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('|', "&#124;")
        .replace(['\n', '\r'], " ")
}
fn heading(text: &str) -> String {
    let mut out = String::new();
    for c in escaped(text).chars() {
        if "\\`*_[]()#!".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
fn code(text: &str) -> String {
    format!("<code>{}</code>", escaped(text))
}
fn json(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("description JSON")
}
fn fence(language: &str, text: &str) -> String {
    // Modeled strings may themselves contain Markdown fences.
    let width = text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
        .max(2)
        + 1;
    let fence = "`".repeat(width);
    format!("{fence}{language}\n{text}\n{fence}\n\n")
}
fn docs(item: &Value) -> String {
    format!("{}\n", string(&item["documentation"]["html"]))
}
fn badge(item: &Value) -> &'static str {
    if item["internal"] == true {
        " **Internal**"
    } else {
        ""
    }
}
fn notes(item: &Value) -> String {
    let mut out = String::new();
    if item["internal"] == true {
        out.push_str("> **Internal API.**\n\n");
    }
    let n = &item["notes"];
    if !n["deprecated"].is_null() {
        let d = &n["deprecated"];
        let mut text = "Deprecated".to_owned();
        if let Some(since) = d["since"].as_str() {
            text.push_str(&format!(" since {since}"));
        }
        if let Some(message) = d["message"].as_str() {
            text.push_str(&format!(": {message}"));
        }
        out.push_str(&format!(
            "> **Deprecated.** {}\n\n",
            documentation::html(&text).trim().replace('\n', " ")
        ));
    }
    if n["unstable"] == true {
        out.push_str("> **Unstable API.** This API may change.\n\n");
    }
    // `@since` is the author's fallback for what is older than every
    // supported release (decision 77).
    if let Some(since) = n["since"].as_str()
        && item["history"]["added"].is_null()
    {
        out.push_str(&format!("Since {}.\n\n", code(since)));
    }
    if let Some(links) = n["externalDocs"].as_object() {
        for (name, url) in links {
            out.push_str(&documentation::html(&external_link(name, string(url))));
        }
        if !links.is_empty() {
            out.push('\n');
        }
    }
    out
}
/// An `@externalDocs` link as Markdown: its name is model text, escaped so
/// it opens no link, tag or heading of its own, and its URL a web or mail
/// address with what would end the link percent-encoded; a name alone
/// where the URL is neither.
fn external_link(name: &str, url: &str) -> String {
    use crucible_codegen_common::guide::{markdown_text, markdown_url};
    match markdown_url(url) {
        Some(url) => format!("[{}](<{url}>)", markdown_text(name)),
        None => markdown_text(name),
    }
}
fn operation_path(item: &Value) -> String {
    format!("operations/API_{}.md", string(&item["name"]))
}
fn shape_path(item: &Value) -> String {
    format!("shapes/{}.md", string(&item["name"]))
}
/// Model text (a release name is any visible ASCII) as Markdown text that
/// reads back exactly as written: every ASCII punctuation character
/// backslash-escaped, which CommonMark allows for any of them and renders
/// without the backslash, so none opens or closes a link, emphasis, a code
/// span, an entity or raw HTML. Line breaks become spaces.
fn literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' | '\r' => out.push(' '),
            c if c.is_ascii_punctuation() => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}
fn link(label: &str, path: &str) -> String {
    format!("[{}]({path})", literal(label))
}
/// A release name in `<code>`. Markdown still parses inline content between
/// raw HTML tags (a link or emphasis there is live) and shows a backslash
/// escape there as typed, so every ASCII punctuation character is a numeric
/// character reference instead, which both Markdown and HTML read as the
/// character itself.
fn release_code(name: &str) -> String {
    let mut out = String::from("<code>");
    for c in name.chars() {
        match c {
            '\n' | '\r' => out.push(' '),
            c if c.is_ascii_punctuation() => out.push_str(&format!("&#{};", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push_str("</code>");
    out
}
/// A link to a release's changelog from a page at `prefix` from `src/`:
/// every link to a release goes through here.
fn release_link(name: &str, prefix: &str) -> String {
    link(name, &format!("{prefix}{}", release_path(name)))
}

pub(super) fn render(service: &Service, d: &Description) -> Vec<File> {
    let mut summary = "# Summary\n\n- [Service overview](index.md)\n".to_owned();
    let mut index = format!(
        "# {}\n\n{}{}Service version: {}.\n\n## Protocol\n\nWire protocol: {}.\n\n",
        heading(string(&d.service["title"])),
        notes(&d.service),
        docs(&d.service),
        release_code(string(&d.service["version"])),
        code(string(&d.service["protocol"]))
    );
    if let Some(header) = version_header(d) {
        let which = match older_release(d) {
            Some((documented, current)) => format!(
                ". This reference documents release {}; the current release is {}.",
                release_code(documented),
                release_code(current)
            ),
            None => ", the current release.".to_owned(),
        };
        index.push_str(&format!(
            "Every request names the release it is written for in a header: {}{which} {}\n\n",
            release_code(&header),
            VERSION_NOTE
        ));
    }
    if chained(d) {
        index.push_str(&format!(
            "{} lists every supported release and what changed in each.\n\n",
            link("Releases", "releases.md")
        ));
    }
    index.push_str("## Authentication\n\n");
    if list(&d.service["auth"]).is_empty() {
        index.push_str("No default authentication schemes.\n\n");
    }
    for scheme in list(&d.service["auth"]) {
        index.push_str(&format!("- {}\n", scheme_text(d, string(scheme))));
    }
    if !list(&d.service["auth"]).is_empty() {
        index.push_str("\nAn operation that takes no credential says so on its page.\n");
    }
    if !list(&d.service["permissions"]).is_empty() {
        index.push_str(&format!(
            "\nOperations name what the caller must hold on which resource; {} lists every permission.\n",
            link("Permissions", "permissions.md")
        ));
    }
    index.push_str("\n## Service errors\n\n");
    index.push_str(&errors(d, &d.service["errors"], ""));
    index.push_str("## Operations\n\n");
    for resource in d.resources.values() {
        let name = string(&resource["name"]);
        index.push_str(&format!(
            "### {name}{}\n\n{}{}",
            badge(resource),
            notes(resource),
            docs(resource)
        ));
        let lifecycle: Vec<_> = ["create", "put", "read", "update", "delete", "list"]
            .into_iter()
            .filter_map(|slot| {
                let id = resource["lifecycle"][slot].as_str()?;
                Some((slot, d.operations.get(id)?))
            })
            .collect();
        // A resource with custom actions only has no lifecycle to tabulate.
        if !lifecycle.is_empty() {
            index.push_str("| Lifecycle | Operation |\n|---|---|\n");
            for (slot, op) in lifecycle {
                index.push_str(&format!(
                    "| {slot} | {}{} |\n",
                    link(string(&op["name"]), &operation_path(op)),
                    badge(op)
                ));
            }
            index.push('\n');
        }
        summary.push_str(&format!("- {}\n", escaped(name)));
        for id in list(&resource["operations"]) {
            if let Some(op) = d.operations.get(string(id)) {
                let row = format!(
                    "{}{}",
                    link(string(&op["name"]), &operation_path(op)),
                    badge(op)
                );
                index.push_str(&format!("- {row}\n"));
                summary.push_str(&format!("  - {row}\n"));
            }
        }
        index.push('\n');
    }
    let unbound: Vec<_> = d
        .operations
        .values()
        .filter(|op| list(&op["resources"]).is_empty())
        .collect();
    if !unbound.is_empty() {
        summary.push_str("- Service operations\n");
        index.push_str("### Service operations\n\n");
        for op in unbound {
            let row = format!(
                "{}{}",
                link(string(&op["name"]), &operation_path(op)),
                badge(op)
            );
            index.push_str(&format!("- {row}\n"));
            summary.push_str(&format!("  - {row}\n"));
        }
        index.push('\n');
    }
    let mut files = Vec::new();
    if let Some(page) = permissions_page(service, d) {
        summary.push_str("- [Permissions](permissions.md)\n");
        files.push(File {
            path: "src/permissions.md".into(),
            contents: page,
        });
    }
    summary.push_str("- Data types\n");
    for op in d.operations.values() {
        files.push(File {
            path: format!("src/{}", operation_path(op)),
            contents: operation(d, op),
        });
    }
    for (id, shape) in &d.shapes {
        if crate::builtin(id) {
            continue;
        }
        summary.push_str(&format!(
            "  - {}{}\n",
            link(string(&shape["name"]), &shape_path(shape)),
            badge(shape)
        ));
        files.push(File {
            path: format!("src/{}", shape_path(shape)),
            contents: shape_page(d, shape),
        });
    }
    if chained(d) {
        summary.push_str("- [Releases](releases.md)\n");
        files.push(File {
            path: "src/releases.md".into(),
            contents: releases_page(d),
        });
        let releases = list(&d.versions["releases"]);
        let guides = upgrade::pairs(d);
        for (index, release) in releases.iter().enumerate() {
            let name = string(&release["name"]);
            summary.push_str(&format!("  - {}\n", release_link(name, "")));
            files.push(File {
                path: format!("src/{}", release_path(name)),
                contents: release_page(d, release),
            });
            // Each upgrade guide under the release it starts from.
            for &(from, to) in guides.iter().filter(|(from, _)| *from == index) {
                let target = string(&releases[to]["name"]);
                let path = upgrade::path(name, target);
                summary.push_str(&format!(
                    "    - {}\n",
                    link(&format!("Upgrading to {target}"), &path)
                ));
                files.push(File {
                    path: format!("src/{path}"),
                    contents: upgrade::page(d, from, to),
                });
            }
        }
    }
    files.push(File {
        path: "src/SUMMARY.md".into(),
        contents: summary,
    });
    files.push(File {
        path: "src/index.md".into(),
        contents: index,
    });
    for file in &mut files {
        let depth = file.path.matches('/').count() - 1;
        file.contents = doc_links(d, &file.contents, &"../".repeat(depth));
        // Every page a reader can land on says which release it documents;
        // the table of contents is mdBook's navigation, not a page.
        if file.path != "src/SUMMARY.md"
            && let Some(banner) = release_banner(d, depth)
        {
            // Under the page's title, which every page starts with, and
            // one blank line before what followed it.
            let (title, rest) = file
                .contents
                .split_once('\n')
                .unwrap_or((&file.contents, ""));
            file.contents = format!("{title}\n\n{banner}{}", rest.trim_start_matches('\n'));
        }
    }
    let agents = llms(d, &files);
    files.extend(agents);
    files
}

/// The first sentence of an item's documentation, as one line of plain
/// text: what an index line says about the page it links.
fn summary(item: &Value) -> String {
    let text = string(&item["documentation"]["text"]);
    let paragraph = text.split("\n\n").next().unwrap_or_default();
    let line = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
    // A sentence ends at a period before a capital, so "Payments Inc.
    // reports" and "e.g. a city" stay whole.
    let end = line.match_indices(". ").map(|(at, _)| at).find(|&at| {
        line[at + 2..]
            .chars()
            .next()
            .is_some_and(char::is_uppercase)
    });
    plain(match end {
        Some(end) => &line[..=end],
        None => &line,
    })
}

/// Model text as inert Markdown text, for `llms.txt`, which an agent reads
/// as written: `<`, `>` and `&` as character references, which open no tag,
/// comment or entity, and every other ASCII punctuation character
/// backslash-escaped, so none opens a link, emphasis or code span.
pub(crate) fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '\n' | '\r' => out.push(' '),
            c if c.is_ascii_punctuation() => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

/// An index line's description: the page's own facts, then its summary
/// and badges.
fn described(facts: &str, item: &Value) -> String {
    let mut parts = vec![facts.to_owned()];
    let summary = summary(item);
    if !summary.is_empty() {
        parts.push(summary);
    }
    if item["internal"] == true {
        parts.push("Internal.".into());
    }
    if !item["notes"]["deprecated"].is_null() {
        parts.push("Deprecated.".into());
    }
    parts.join(" ")
}

/// `llms.txt` and `llms-full.txt` (llmstxt.org): an index of the pages for
/// an agent, which fetches Markdown rather than the single-page reference,
/// and every page in one file. Links are relative to the output's root,
/// where the pages sit under `src/` beside `index.html`.
fn llms(d: &Description, pages: &[File]) -> [File; 2] {
    let title = string(&d.service["title"]);
    let mut about = summary(&d.service);
    if about.is_empty() {
        about = format!("The API reference of {}.", plain(title));
    }
    // The title is one heading's text: `plain` escapes what `heading`
    // does, and a URL's `:` too.
    let mut out = format!("# {}\n\n> {about}\n\n", plain(title));
    let mut facts = vec![format!(
        "Every page below is Markdown. Requests are {} over HTTP; each operation's page gives its method and URI, its headers, its request and response with an example, its errors and its pagination.",
        if d.service["protocol"] == "crucibleJson" {
            "JSON".to_owned()
        } else {
            format!("`{}`", string(&d.service["protocol"]))
        }
    )];
    let schemes: Vec<&str> = list(&d.service["auth"])
        .iter()
        .filter_map(|scheme| scheme_header(d, string(scheme)))
        .collect();
    if !schemes.is_empty() {
        facts.push(
            "Send `Authorization: Bearer <token>` unless an operation's page says it takes no credential."
                .into(),
        );
    }
    if let Some((documented, current)) = older_release(d) {
        let sunset = documented_sunset(d).map_or_else(String::new, |date| {
            format!(", which is supported until {}", code_span(date))
        });
        facts.push(format!(
            "This reference documents release {}{sunset}; the current release is {}, which has a reference of its own: [{}]({}llms.txt).",
            code_span(documented),
            code_span(current),
            plain(current),
            current_reference(current)
        ));
    }
    if let Some(header) = version_header(d) {
        // A third party's request names no release (decision 81).
        let signed = d.operations.values().any(|op| !op["sender"].is_null());
        facts.push(format!(
            "Send {} on every request you make{}.",
            code_span(&header),
            if signed {
                "; an operation a third party calls says what its sender sends instead"
            } else {
                ""
            }
        ));
    }
    facts.push("`describe.json` holds the whole reference as data (`crucible.describe/1`).".into());
    out.push_str(&facts.join(" "));
    out.push_str("\n\n## Overview\n\n");
    out.push_str("- [Service overview](src/index.md): protocol, authentication, service errors, and the operations of each resource\n");
    if pages.iter().any(|page| page.path == "src/permissions.md") {
        out.push_str("- [Permissions](src/permissions.md): every permission an operation can require, by resource type\n");
    }
    out.push_str("\n## Operations\n\n");
    for op in d.operations.values() {
        let mut facts = format!(
            "{}.",
            code_span(&format!("{} {}", string(&op["method"]), string(&op["uri"])))
        );
        if let Some(sender) = op["sender"].as_object() {
            let name = plain(
                sender
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            match sender.get("header").and_then(Value::as_str) {
                Some(header) => facts.push_str(&format!(
                    " Called by {name}, which signs it in the {} header.",
                    code_span(header)
                )),
                None => facts.push_str(&format!(" Called by {name}, which signs it.")),
            }
        }
        out.push_str(&format!(
            "- [{}](src/{}): {}\n",
            string(&op["name"]),
            operation_path(op),
            described(&facts, op)
        ));
    }
    out.push_str("\n## Data types\n\n");
    for (id, shape) in &d.shapes {
        if crate::builtin(id) {
            continue;
        }
        let kind = if shape["error"].is_null() {
            string(&shape["kind"]).to_owned()
        } else {
            format!("error, HTTP {}", shape["error"]["status"])
        };
        out.push_str(&format!(
            "- [{}](src/{}): {}\n",
            string(&shape["name"]),
            shape_path(shape),
            described(&format!("{}.", kind), shape)
        ));
    }
    if chained(d) {
        out.push_str("\n## Releases\n\n- [Releases](src/releases.md): every supported release, oldest first\n");
        for release in list(&d.versions["releases"]) {
            let name = string(&release["name"]);
            out.push_str(&format!(
                "- [{}](src/{}): what changed in {}{}\n",
                literal(name),
                release_path(name),
                literal(name),
                if release["current"] == true {
                    ", the current release"
                } else {
                    ""
                }
            ));
        }
        let releases = list(&d.versions["releases"]);
        for (from, to) in upgrade::pairs(d) {
            let (from, to) = (
                string(&releases[from]["name"]),
                string(&releases[to]["name"]),
            );
            out.push_str(&format!(
                "- [Upgrading from {} to {}](src/{}): every change a client of {} meets at {}, and what was added since\n",
                literal(from),
                literal(to),
                upgrade::path(from, to),
                literal(from),
                literal(to)
            ));
        }
    }
    out.push_str("\n## Optional\n\n");
    out.push_str("- [llms-full.txt](llms-full.txt): every page above in one file\n");
    out.push_str(
        "- [describe.json](describe.json): the whole reference as JSON (`crucible.describe/1`)\n",
    );
    out.push_str(
        "- [changes.json](changes.json): the release chain alone (`crucible.changes/1`)\n",
    );

    // The pages in the index's order, each under a comment naming its file,
    // so a reader can follow a page's relative links from it.
    let page = |path: &str| pages.iter().find(|page| page.path == path);
    let mut order: Vec<String> = vec!["src/index.md".into(), "src/permissions.md".into()];
    order.extend(
        d.operations
            .values()
            .map(|op| format!("src/{}", operation_path(op))),
    );
    order.extend(
        d.shapes
            .iter()
            .filter(|(id, _)| !crate::builtin(id))
            .map(|(_, shape)| format!("src/{}", shape_path(shape))),
    );
    if chained(d) {
        order.push("src/releases.md".into());
        order.extend(
            list(&d.versions["releases"])
                .iter()
                .map(|release| format!("src/{}", release_path(string(&release["name"])))),
        );
        let releases = list(&d.versions["releases"]);
        order.extend(upgrade::pairs(d).into_iter().map(|(from, to)| {
            format!(
                "src/{}",
                upgrade::path(
                    string(&releases[from]["name"]),
                    string(&releases[to]["name"])
                )
            )
        }));
    }
    // Each page starts at a comment naming its file, its `=` runs longer
    // than any in the pages, so no page's text (a code block, an example)
    // can pose as a boundary. Its links are rebased onto the root, where
    // this file is.
    let pages: Vec<&File> = order.iter().filter_map(|path| page(path)).collect();
    let longest = pages
        .iter()
        .flat_map(|page| page.contents.split(|c| c != '=').map(str::len))
        .max()
        .unwrap_or(0);
    let rule = "=".repeat(longest.max(2) + 1);
    let full: Vec<String> = pages
        .iter()
        .map(|page| {
            let dir = page.path.rsplit_once('/').map_or("", |(dir, _)| dir);
            format!(
                "<!-- {rule} {} {rule} -->\n\n{}",
                page.path,
                rebase(page.contents.trim_end(), dir)
            )
        })
        .collect();
    [
        File {
            path: "llms.txt".into(),
            contents: out,
        },
        File {
            path: "llms-full.txt".into(),
            contents: full.join("\n\n"),
        },
    ]
}

/// A page's relative links (Markdown destinations and the `href`s of the
/// documentation's anchors) rewritten from its directory `dir` to the root.
/// An absolute URL, a fragment or a root path is left as it is.
fn rebase(contents: &str, dir: &str) -> String {
    use pulldown_cmark::{Event, Parser, Tag};
    let resolve = |destination: &str| -> Option<String> {
        if destination.is_empty()
            || destination.contains(':')
            || destination.starts_with(['#', '/'])
        {
            return None;
        }
        let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
        for part in destination.split('/') {
            match part {
                "." => {}
                // Past the root, as a link to a reference beside this one
                // goes, the `..` stays.
                ".." if parts.last().is_none_or(|last| *last == "..") => parts.push(".."),
                ".." => {
                    parts.pop();
                }
                part => parts.push(part),
            }
        }
        Some(parts.join("/"))
    };
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for (event, range) in Parser::new(contents).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                // The generator's links are inline: `[label](destination)`.
                let source = &contents[range.clone()];
                let written = format!("]({dest_url})");
                if let Some(at) = source.rfind(&written)
                    && let Some(rebased) = resolve(&dest_url)
                {
                    let start = range.start + at + 2;
                    edits.push((start..start + dest_url.len(), rebased));
                }
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                let source = &contents[range.clone()];
                for (at, _) in source.match_indices("href=\"") {
                    let start = at + "href=\"".len();
                    let Some(length) = source[start..].find('"') else {
                        continue;
                    };
                    let destination = &source[start..start + length];
                    if let Some(rebased) = resolve(destination) {
                        let start = range.start + start;
                        edits.push((start..start + length, rebased));
                    }
                }
            }
            _ => {}
        }
    }
    edits.sort_by_key(|(range, _)| range.start);
    edits.dedup_by_key(|(range, _)| range.start);
    let mut out = String::with_capacity(contents.len());
    let mut at = 0;
    for (range, text) in edits {
        if range.start < at {
            continue;
        }
        out.push_str(&contents[at..range.start]);
        out.push_str(&text);
        at = range.end;
    }
    out.push_str(&contents[at..]);
    out
}

/// The pages' doc links (decision 80): each `<a data-crucible-ref="<id>">`
/// the description's documentation HTML carries becomes a link to the page
/// for the id, relative to the page at `prefix`, which mdBook rewrites to
/// its `.html` like any other: an operation's, a data type's, the overview
/// (for the service and a resource, which has a section there), and for a
/// member or enum value its type's page, or the operation's for a member of
/// its input or output. An id with no page loses the anchor, keeping its
/// text.
fn doc_links(d: &Description, contents: &str, prefix: &str) -> String {
    const OPEN: &str = "<a data-crucible-ref=\"";
    if !contents.contains(OPEN) {
        return contents.to_owned();
    }
    let mut out = String::with_capacity(contents.len());
    let mut rest = contents;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + OPEN.len()..];
        let Some(quote) = after.find("\">") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let unescape = |text: &str| {
            text.replace("&quot;", "\"")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
        };
        // `<a data-crucible-ref="ID">`, or with ` data-crucible-operation="OP"`.
        let id = unescape(&after[..quote]);
        let marker = "\" data-crucible-operation=\"";
        let (operation, body) = match after[quote..].strip_prefix(marker) {
            Some(rest) => match rest.find("\">") {
                Some(end) => (Some(unescape(&rest[..end])), &rest[end + 2..]),
                None => (None, &after[quote + 2..]),
            },
            None => (None, &after[quote + 2..]),
        };
        // Links do not nest, so the next `</a>` closes this one.
        let end = body.find("</a>").unwrap_or(body.len());
        match page(d, &id, operation.as_deref()) {
            Some(page) => out.push_str(&format!(
                "<a href=\"{prefix}{}\">{}</a>",
                escaped(&page),
                &body[..end]
            )),
            None => out.push_str(&body[..end]),
        }
        rest = body.get(end + 4..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// The page, relative to `src/`, a doc link's id names.
/// `operation` is the operation an `[Operation.member]` link names.
fn page(d: &Description, id: &str, operation: Option<&str>) -> Option<String> {
    if Some(id) == d.service["id"].as_str() {
        return Some("index.md".into());
    }
    if let Some(op) = d.operations.get(id) {
        return Some(operation_path(op));
    }
    if d.resources.contains_key(id) {
        return Some("index.md".into());
    }
    let (shape, member) = match id.split_once('$') {
        Some((shape, _)) => (shape, true),
        None => (id, false),
    };
    if member {
        let carries = |op: &&Value| {
            op["input"].as_str() == Some(shape) || op["output"].as_str() == Some(shape)
        };
        let owner = operation
            .and_then(|id| d.operations.get(id))
            .filter(carries)
            .or_else(|| d.operations.values().find(carries));
        if let Some(op) = owner {
            return Some(operation_path(op));
        }
    }
    d.shapes
        .get(shape)
        .filter(|_| !crate::builtin(shape))
        .map(shape_path)
}

fn errors(d: &Description, ids: &Value, prefix: &str) -> String {
    if list(ids).is_empty() {
        return "No modeled errors.\n\n".into();
    }
    let mut out =
        "| Error | Code | HTTP status | Fault | Retryable | Description |\n|---|---|---|---|---|---|\n"
            .to_owned();
    let mut headers = std::collections::BTreeSet::new();
    // Errors whose body is a payload: their code is in the header alone.
    let mut payloads = Vec::new();
    for id in list(ids) {
        let shape = &d.shapes[string(id)];
        let e = &shape["error"];
        let retry = if e["retryable"].is_null() {
            "No"
        } else if e["retryable"]["throttling"] == true {
            "Yes (throttling)"
        } else {
            "Yes"
        };
        if let Some(header) = e["discriminatorHeader"].as_str() {
            headers.insert(header);
        }
        if e["bodyDiscriminator"].is_null() || !e["bindings"]["payload"].is_null() {
            payloads.push(code(string(&shape["name"])));
        }
        let wire = e["bodyDiscriminator"]
            .as_str()
            .unwrap_or(string(&e["wireCode"]));
        out.push_str(&format!(
            "| {}{} | {} | {} | {} | {retry} | {} |\n",
            link(
                string(&shape["name"]),
                &format!("{prefix}{}", shape_path(shape))
            ),
            badge(shape),
            code(wire),
            e["status"],
            string(&e["fault"]),
            cell_docs(shape, prefix)
        ));
    }
    out.push('\n');
    // How a reader tells the errors apart: the code travels in a header
    // beside the body.
    if let [header] = headers.into_iter().collect::<Vec<_>>()[..] {
        let body = if payloads.is_empty() {
            " and in its JSON body".to_owned()
        } else if payloads.len() < list(ids).len() {
            format!(
                ", and in its JSON body except for {}, whose body carries no code",
                payloads.join(", ")
            )
        } else {
            String::new()
        };
        out.push_str(&format!(
            "An error response names its code in the {} header{body}.\n\n",
            code(header)
        ));
    }
    out
}
/// An item's description cell, on a page at `prefix` from `src/`.
fn cell_docs(item: &Value, prefix: &str) -> String {
    let mut value = string(&item["documentation"]["html"])
        .replace('|', "&#124;")
        .replace('\n', " ");
    if item["internal"] == true {
        value.push_str(" <strong>Internal</strong>");
    }
    if !item["notes"]["deprecated"].is_null() {
        value.push_str(" <strong>Deprecated</strong>");
        let deprecated = &item["notes"]["deprecated"];
        if let Some(since) = deprecated["since"].as_str() {
            value.push_str(&format!(" since {}", code(since)));
        }
        if let Some(message) = deprecated["message"].as_str() {
            value.push_str(&documentation::html(message));
        }
    }
    if item["notes"]["unstable"] == true {
        value.push_str(" <strong>Unstable</strong>");
    }
    if let Some(since) = item["notes"]["since"].as_str()
        && item["history"]["added"].is_null()
    {
        value.push_str(&format!(" Since {}.", code(since)));
    }
    value.push_str(&history_cell(&item["history"], prefix));
    if let Some(links) = item["notes"]["externalDocs"].as_object() {
        for (name, url) in links {
            value.push_str(&documentation::html(&external_link(name, string(url))));
        }
    }
    value.replace('|', "&#124;").replace('\n', " ")
}
fn target(d: &Description, m: &Value) -> String {
    if let Some(shape) = d.shapes.get(string(&m["target"]))
        && !crate::builtin(string(&m["target"]))
    {
        link(
            string(&m["targetName"]),
            &format!("../{}", shape_path(shape)),
        )
    } else {
        code(string(&m["targetType"]))
    }
}
fn constraint_text(value: &Value) -> String {
    let mut fields = Vec::new();
    for key in ["length", "range", "pattern"] {
        if !value[key].is_null() {
            fields.push(format!("{key}: {}", code(&value[key].to_string())));
        }
    }
    if value["uniqueItems"] == true {
        fields.push("Unique items".into());
    }
    if fields.is_empty() {
        "—".into()
    } else {
        fields.join("; ")
    }
}
fn members(
    d: &Description,
    members: impl Iterator<Item = Value>,
    bindings: Option<&Value>,
) -> String {
    let mut out="| Member | Wire location | Type | Required | Default | Constraints | Description |\n|---|---|---|---|---|---|---|\n".to_owned();
    for m in members {
        let name = string(&m["name"]);
        let location =
            bindings.map_or_else(|| string(&m["wireName"]).to_owned(), |b| location(b, &m));
        let default = if m["default"].is_null() {
            "—".into()
        } else {
            code(&m["default"].to_string())
        };
        let mut detail = cell_docs(&m, "../");
        if m["sensitive"] == true {
            detail.push_str(" <strong>Sensitive</strong>");
        }
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            code(name),
            code(&location),
            target(d, &m),
            if m["required"] == true { "Yes" } else { "No" },
            default,
            constraint_text(&m["constraints"]),
            detail
        ));
    }
    out.push('\n');
    out
}
fn location(b: &Value, m: &Value) -> String {
    let name = string(&m["name"]);
    if list(&b["labels"]).iter().any(|n| string(n) == name) {
        return format!("path: {name}");
    }
    for key in ["query", "headers"] {
        if let Some(binding) = list(&b[key]).iter().find(|v| string(&v["member"]) == name) {
            return format!("{key}: {}", string(&binding["name"]));
        }
    }
    if string(&b["payload"]["member"]) == name {
        return "payload".into();
    }
    format!("body: {}", string(&m["wireName"]))
}
fn message(d: &Description, op: &Value, request: bool) -> String {
    let (title, shape_key, bindings_key) = if request {
        ("Request", "input", "request")
    } else {
        ("Response", "output", "response")
    };
    let b = &op[bindings_key];
    let mut out = format!("## {title} syntax\n\n");
    let mut http = if request {
        format!("{} {} HTTP/1.1", string(&op["method"]), string(&op["uri"]))
    } else {
        format!("HTTP/1.1 {}", op["status"])
    };
    if request && !list(&b["query"]).is_empty() {
        let mut uri = string(&op["uri"]).to_owned();
        for query in list(&b["query"]) {
            let separator = if uri.contains('?') { '&' } else { '?' };
            uri.push_str(&format!(
                "{separator}{}={{{}}}",
                string(&query["name"]),
                string(&query["member"])
            ));
        }
        http = format!("{} {uri} HTTP/1.1", string(&op["method"]));
    }
    if let Some(headers) = op[if request {
        "protocolHeaders"
    } else {
        "responseProtocolHeaders"
    }]
    .as_object()
    {
        for (name, value) in headers {
            http.push_str(&format!("\n{name}: {}", string(value)));
        }
    }
    // What every request carries besides its modeled headers: the
    // credential and the release it is written for.
    if request {
        for scheme in list(&op["auth"]) {
            if let Some(header) = scheme_header(d, string(scheme)) {
                http.push_str(&format!("\n{header}"));
            }
        }
        // A third party's request names no release of this API.
        if let Some(header) = version_header(d).filter(|_| op["sender"].is_null()) {
            http.push_str(&format!("\n{header}"));
        }
    }
    for header in list(&b["headers"]) {
        http.push_str(&format!(
            "\n{}: {{{}}}",
            string(&header["name"]),
            string(&header["member"])
        ));
    }
    for header in list(&b["prefixHeaders"]) {
        http.push_str(&format!(
            "\n{}{{key}}: {{{}}}",
            string(&header["name"]),
            string(&header["member"])
        ));
    }
    if !b["payload"].is_null() {
        http.push_str(&format!("\n\n{{{}}}", string(&b["payload"]["member"])));
    }
    out.push_str(&fence("http", &http));
    if request && version_header(d).is_some() && op["sender"].is_null() {
        out.push_str(&format!(
            "{} names the release this page describes. {}\n\n",
            code(string(&d.versions["carrier"]["name"])),
            VERSION_NOTE
        ));
    }
    if let Some(shape) = d.shapes.get(string(&op[shape_key])) {
        let all = list(&shape["members"]);
        let parameters: Vec<_> = all
            .iter()
            .filter(|m| {
                !list(&b["body"])
                    .iter()
                    .any(|n| string(n) == string(&m["name"]))
                    && string(&b["payload"]["member"]) != string(&m["name"])
            })
            .cloned()
            .collect();
        if !parameters.is_empty() {
            out.push_str(&format!("### {title} parameters\n\n"));
            out.push_str(&members(d, parameters.into_iter(), Some(b)));
        }
        let body: Vec<_> = all
            .iter()
            .filter(|m| {
                list(&b["body"])
                    .iter()
                    .any(|n| string(n) == string(&m["name"]))
                    || string(&b["payload"]["member"]) == string(&m["name"])
            })
            .cloned()
            .collect();
        if !body.is_empty() {
            out.push_str(&format!("### {title} body\n\n"));
            let syntax: serde_json::Map<String, Value> = body
                .iter()
                .map(|m| {
                    (
                        string(&m["wireName"]).into(),
                        Value::String(format!("<{}>", string(&m["targetType"]))),
                    )
                })
                .collect();
            if b["payload"].is_null() {
                out.push_str(&fence("json", &json(&Value::Object(syntax))));
            }
            out.push_str(&members(d, body.into_iter(), Some(b)));
        }
        if all.is_empty() {
            out.push_str("No modeled members.\n\n");
        }
    } else {
        out.push_str("No modeled body.\n\n");
    }
    out
}

fn example_message(message: &Value, request: bool, status: &Value) -> String {
    let mut wire = if request {
        format!(
            "{} {} HTTP/1.1",
            string(&message["method"]),
            string(&message["uri"])
        )
    } else {
        format!(
            "HTTP/1.1 {}",
            if message["responseCode"].is_null() {
                status
            } else {
                &message["responseCode"]
            }
        )
    };
    if let Some(headers) = message["headers"].as_object() {
        for (name, value) in headers {
            wire.push_str(&format!("\n{name}: {}", string(value)));
        }
    }
    if !message["body"].is_null() {
        wire.push_str(&format!("\n\n{}", json(&message["body"])));
    }
    if !message["payload"].is_null() {
        let payload = &message["payload"];
        wire.push_str(&format!(
            "\n\n{}",
            if payload["encoding"] == "base64" {
                format!("<binary payload, base64: {}>", string(&payload["value"]))
            } else if payload["kind"] == "string" {
                string(&payload["value"]).to_owned()
            } else {
                json(&payload["value"])
            }
        ));
    }
    fence("http", &wire)
}
fn operation(d: &Description, op: &Value) -> String {
    let mut out = format!(
        "# {}\n\n{}{}{}",
        string(&op["name"]),
        notes(op),
        docs(op),
        history_line(&op["history"], "../")
    );
    out.push_str(&format!(
        "Protocol: {}.\n\n## Authentication\n\n",
        code(string(&op["protocol"]))
    ));
    out.push_str(&authentication(d, op));
    out.push_str(&authorization(op));
    out.push_str(&message(d, op, true));
    out.push_str(&message(d, op, false));
    out.push_str("## Errors\n\n");
    out.push_str(&errors(d, &op["errors"], "../"));
    out.push_str("## Idempotency and retries\n\n");
    out.push_str(&format!(
        "Readonly: {}. Idempotent: {}.\n\n",
        op["readonly"], op["idempotent"]
    ));
    if let Some(token) = op["idempotencyToken"].as_str() {
        out.push_str(&format!(
            "Idempotency token: {}. {}\n\n",
            code(token),
            crucible_codegen_common::docs::IDEMPOTENCY_TOKEN_NOTE
        ));
    }
    if !op["pagination"].is_null() {
        out.push_str("## Pagination\n\n| Role | Member path |\n|---|---|\n");
        for key in ["inputToken", "outputToken", "items", "pageSize"] {
            if let Some(value) = op["pagination"][key].as_str() {
                out.push_str(&format!("| {key} | {} |\n", code(value)));
            }
        }
        out.push_str(
            "\nFollow the output token until absent or empty; reject repeated tokens.\n\n",
        );
    }
    if !list(&op["examples"]).is_empty() {
        out.push_str("## Examples\n\n");
        for e in list(&op["examples"]) {
            out.push_str(&format!(
                "### {}\n\n{}",
                heading(string(&e["title"])),
                docs(e)
            ));
            if e["allowConstraintErrors"] == true {
                out.push_str("This example intentionally permits constraint violations.\n\n");
            }
            for (key, label) in [
                ("input", "Request"),
                ("output", "Response"),
                ("error", "Error response"),
            ] {
                if !e[key].is_null() {
                    out.push_str(&format!("{label}:\n\n"));
                    let (message, status) = if key == "error" {
                        (&e[key]["message"], &e[key]["status"])
                    } else {
                        (&e[key], &op["status"])
                    };
                    out.push_str(&example_message(message, key == "input", status));
                }
            }
        }
    } else if !op["sample"].is_null() {
        // Without an example, the sample the model implies, marked as such.
        out.push_str(&format!("## Generated sample\n\n{}\n\n", GENERATED_NOTE));
        // The same warning the static reference shows.
        let fallbacks: Vec<String> = list(&op["sample"]["fallbacks"])
            .iter()
            .map(|f| code(string(&f["path"])))
            .collect();
        if !fallbacks.is_empty() {
            out.push_str(&format!(
                "Placeholders that do not meet their constraints: {}.\n\n",
                fallbacks.join(", ")
            ));
        }
        for (key, label) in [("input", "Request"), ("output", "Response")] {
            out.push_str(&format!("{label}:\n\n"));
            out.push_str(&example_message(
                &op["sample"][key],
                key == "input",
                &op["status"],
            ));
        }
    }
    out
}
/// Markdown from the model (a permission's description) as one table
/// cell, its doc links resolved as the description's documentation is.
fn cell(service: &Service, markdown: &str) -> String {
    table_cell(&documentation::html_linked(service, markdown))
}

/// Documentation HTML as one table cell.
fn table_cell(html: &str) -> String {
    html.trim().replace('|', "&#124;").replace('\n', " ")
}

/// Who calls a sender-signed operation (decision 81) and where its
/// signature travels, or nothing for an operation clients call.
fn sender(op: &Value) -> Option<String> {
    let sender = op["sender"].as_object()?;
    let name = literal(
        sender
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let header = sender
        .get("header")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(format!(
        "Called by {name}, not by clients of this API. It takes no credential: {name} signs each request, and the service verifies the signature in the {} header.",
        code(header)
    ))
}

/// How the caller authenticates: each effective scheme as the header it
/// sends, or that the operation takes no credential.
fn authentication(d: &Description, op: &Value) -> String {
    if let Some(sender) = sender(op) {
        return format!("{sender}\n\n");
    }
    let schemes = list(&op["auth"]);
    if schemes.is_empty() {
        return "None: this operation takes no credential.\n\n".into();
    }
    let mut out = String::new();
    for scheme in schemes {
        out.push_str(&format!("- {}\n", scheme_text(d, string(scheme))));
    }
    out.push('\n');
    out
}

/// What the caller must hold (docs/permissions.md §6.3), or nothing for an
/// operation that declares nothing.
fn authorization(op: &Value) -> String {
    let requirements = &op["requirements"];
    let Some(kind) = requirements["kind"].as_str() else {
        return String::new();
    };
    let mut out = "## Authorization\n\n".to_owned();
    match kind {
        "none" => out.push_str("Requires no permission.\n\n"),
        // Only the server audience keeps a pending operation's reason.
        "pending" => out.push_str(&format!(
            "Requirements not declared yet; the service checks them itself: {}\n\n",
            escaped(string(&requirements["reason"]))
        )),
        "internal" => out.push_str("Requires permissions internal to the service.\n\n"),
        _ => {
            for sentence in list(&op["requirementDocumentation"]) {
                out.push_str(string(&sentence["html"]));
                out.push('\n');
            }
            out.push_str(
                "| Mode | Permission | Resource type | Bindings | When | Notes |\n|---|---|---|---|---|---|\n",
            );
            for clause in list(&requirements["clauses"]) {
                let bindings: Vec<String> = list(&clause["bindings"])
                    .iter()
                    .map(|b| {
                        format!(
                            "{} = {}",
                            code(string(&b["identifier"])),
                            code(string(&b["value"]))
                        )
                    })
                    .collect();
                // `When` is the condition alone; a clause's own documentation
                // (what a handler checks, and when) is a note beside it.
                let when = clause["when"].as_str().map(code).unwrap_or_default();
                let notes = clause["renderedDocumentation"]["html"]
                    .as_str()
                    .map(table_cell)
                    .unwrap_or_default();
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} |\n",
                    string(&clause["mode"]),
                    code(string(&clause["permission"])),
                    code(string(&clause["resourceType"])),
                    bindings.join(", "),
                    when,
                    notes
                ));
            }
            out.push('\n');
        }
    }
    out
}

/// `permissions.md`: the registry by resource type, a table per tier
/// (docs/permissions.md §6.3). Internal entries exist only in a
/// server-audience description.
fn permissions_page(service: &Service, d: &Description) -> Option<String> {
    let registry = list(&d.service["permissions"]);
    if registry.is_empty() {
        return None;
    }
    let mut out = "# Permissions\n\nWhat a grant names: a resource type and a permission value. Values are compared case-sensitively; a deprecated alias means the same as the value it names.\n\n".to_owned();
    for resource in registry {
        out.push_str(&format!(
            "## {}\n\nIdentifiers: {}.\n\n",
            code(string(&resource["resourceType"])),
            list(&resource["identifiers"])
                .iter()
                .map(|id| code(string(id)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let permissions = list(&resource["permissions"]);
        for (tier, title, intro) in [
            ("public", "Permissions", ""),
            (
                "permissionOnly",
                "Permission-only",
                "Required by no operation: the service's policy checks them.\n\n",
            ),
            (
                "notGrantable",
                "Bundles",
                "Not grantable on their own: the service's policy confers them whole.\n\n",
            ),
            (
                "internal",
                "Internal",
                "The server's own vocabulary: never in a client, a public reference or a refusal.\n\n",
            ),
        ] {
            let rows: Vec<&Value> = permissions.iter().filter(|p| p["tier"] == tier).collect();
            if rows.is_empty() {
                continue;
            }
            out.push_str(&format!(
                "### {title}\n\n{intro}| Permission | Description |\n|---|---|\n"
            ));
            for p in rows {
                let mut description = cell(service, string(&p["description"]));
                if let Some(primary) = p["aliasOf"].as_str() {
                    description.push_str(&format!(
                        " <strong>Deprecated</strong> alias of {}.",
                        code(primary)
                    ));
                } else if p["deprecated"] == true {
                    description.push_str(" <strong>Deprecated</strong>");
                }
                out.push_str(&format!(
                    "| {} | {} |\n",
                    code(string(&p["value"])),
                    description.trim()
                ));
            }
            out.push('\n');
        }
    }
    Some(out)
}

fn shape_page(d: &Description, shape: &Value) -> String {
    let mut out = format!(
        "# {}\n\n{}{}{}Type: {}.\n\n",
        string(&shape["name"]),
        notes(shape),
        docs(shape),
        history_line(&shape["history"], "../"),
        code(string(&shape["kind"]))
    );
    if shape["sensitive"] == true {
        out.push_str("**Sensitive:** avoid logging this value.\n\n");
    }
    if !list(&shape["members"]).is_empty() {
        out.push_str("## Members\n\n");
        out.push_str(&members(d, list(&shape["members"]).iter().cloned(), None));
        if shape["kind"] == "union" {
            out.push_str("A union carries exactly one member.\n\n");
        }
    }
    if !list(&shape["values"]).is_empty() {
        out.push_str("## Values\n\n| Name | Wire value | Description |\n|---|---|---|\n");
        for value in list(&shape["values"]) {
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                code(string(&value["name"])),
                code(&value["value"].to_string()),
                cell_docs(value, "../")
            ));
        }
        out.push('\n');
    }
    if !shape["constraints"].is_null() {
        out.push_str(&format!(
            "Constraints: {}.\n\n",
            constraint_text(&shape["constraints"])
        ));
    }
    out
}

/// A release's changelog page, relative to `src/`: `releases/<file>.md`,
/// with [`release_file`] naming the file.
fn release_path(name: &str) -> String {
    format!("releases/{}.md", release_file(name))
}

/// A release's name as one file name every platform accepts, and a link
/// carries unchanged: letters, digits, `.`, `-` and `+` are kept, and any
/// other byte is `_` and its two hex digits, `_` itself included, so two
/// names never share a file. A leading or trailing `.`, and a name Windows
/// reserves for a device (`con`, `nul`, `com1`...), have their first
/// character escaped too. A release name is any visible ASCII.
pub(crate) fn release_file(name: &str) -> String {
    const DEVICES: &[&str] = &[
        "con", "prn", "aux", "nul", "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
        "com8", "com9", "lpt0", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8",
        "lpt9",
    ];
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let escape_first = DEVICES.contains(&stem.as_str()) || name.starts_with('.');
    let last = name.len().saturating_sub(1);
    let mut out = String::with_capacity(name.len());
    for (index, byte) in name.bytes().enumerate() {
        let kept = byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+');
        let escaped = !kept || (index == 0 && escape_first) || (index == last && byte == b'.');
        if escaped {
            out.push_str(&format!("_{byte:02X}"));
        } else {
            out.push(char::from(byte));
        }
    }
    out
}

/// Whether the chain has more than the current release: only then does the
/// reference have release pages to link (the JSON's `versions` is there
/// either way).
fn chained(d: &Description) -> bool {
    list(&d.versions["releases"]).len() > 1
}

/// An element's dates (decision 77), each release linked to its changelog
/// from a page at `prefix`: "Added in v2.", "Changed in v3.", and the
/// releases that cannot see it. Empty for an element older than every
/// supported release that no migration changed.
fn dates(history: &Value, prefix: &str) -> Vec<String> {
    let releases = |key: &str| {
        list(&history[key])
            .iter()
            .map(|name| release_link(string(name), prefix))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut parts = Vec::new();
    if let Some(added) = history["added"].as_str() {
        parts.push(format!("Added in {}.", release_link(added, prefix)));
    }
    if !list(&history["changed"]).is_empty() {
        parts.push(format!("Changed in {}.", releases("changed")));
    }
    if !list(&history["missing"]).is_empty() {
        parts.push(format!("Clients of {} do not see it.", releases("missing")));
    }
    parts
}

/// The dates under a page's title.
fn history_line(history: &Value, prefix: &str) -> String {
    let parts = dates(history, prefix);
    if parts.is_empty() {
        String::new()
    } else {
        format!("{}\n\n", parts.join(" "))
    }
}

/// The same dates inside a table cell.
fn history_cell(history: &Value, prefix: &str) -> String {
    dates(history, prefix)
        .iter()
        .map(|part| format!(" {part}"))
        .collect()
}

/// An id as the model names it: `Shape`, `Shape.member`, `Operation`.
fn label(id: &str) -> String {
    id.split_once('#')
        .map_or(id, |(_, name)| name)
        .replace('$', ".")
}

/// The element `id` in code, linked to its page from a release page when
/// the description has one.
fn element(d: &Description, id: &Value) -> String {
    let id = string(id);
    match page(d, id, None) {
        Some(page) => format!("<a href=\"../{}\">{}</a>", escaped(&page), code(&label(id))),
        None => code(&label(id)),
    }
}

/// A type by id, as a member table shows it: linked to its page, or the
/// kind of a prelude type.
fn type_of(d: &Description, id: &Value) -> String {
    let kind = d
        .shapes
        .get(string(id))
        .map_or(Value::Null, |s| s["kind"].clone());
    target(
        d,
        &serde_json::json!({"target": id, "targetName": label(string(id)), "targetType": kind}),
    )
}

/// `releases.md`: every supported release, oldest first.
fn releases_page(d: &Description) -> String {
    let versions = &d.versions;
    let documents = older_release(d).map_or_else(String::new, |(documented, _)| {
        format!(
            " This reference documents release {}.",
            release_code(documented)
        )
    });
    let mut out = format!(
        "# Releases\n\nThe current release is {}.{documents} A client names the release it was built for in the {} header.\n\n| Release | Follows | Sunset | Changes |\n|---|---|---|---|\n",
        release_code(string(&versions["current"])),
        code(string(&versions["carrier"]["name"]))
    );
    for release in list(&versions["releases"]) {
        let name = string(&release["name"]);
        out.push_str(&format!(
            "| {}{} | {} | {} | {} |\n",
            release_link(name, ""),
            if release["current"] == true {
                " **Current**"
            } else {
                ""
            },
            release["from"]
                .as_str()
                .map_or_else(|| "—".to_owned(), release_code),
            release["sunset"]
                .as_str()
                .map_or_else(|| "—".to_owned(), release_code),
            list(&release["changes"]).len()
        ));
    }
    out.push('\n');
    out
}

/// A release's changelog: its migration's documentation, and each change
/// with what a client of the release before it sees instead.
fn release_page(d: &Description, release: &Value) -> String {
    let name = string(&release["name"]);
    let mut out = format!("# Release {}\n\n", heading(name));
    let mut facts = Vec::new();
    if release["current"] == true {
        facts.push("The current release.".to_owned());
    }
    match release["from"].as_str() {
        Some(from) => facts.push(format!("Follows {}.", release_link(from, "../"))),
        None => facts.push("The oldest supported release.".to_owned()),
    }
    if let Some(sunset) = release["sunset"].as_str() {
        facts.push(format!("Supported until {}.", release_code(sunset)));
    }
    out.push_str(&format!("{}\n\n", facts.join(" ")));
    if !string(&release["documentation"]["html"]).is_empty() {
        out.push_str(&docs(release));
    }
    if !string(&release["sunsetDocumentation"]["html"]).is_empty() {
        out.push_str(&format!(
            "## Sunset\n\n{}",
            docs(&json_doc(&release["sunsetDocumentation"]))
        ));
    }
    if !list(&release["retired"]).is_empty() {
        let retired: Vec<String> = list(&release["retired"])
            .iter()
            .map(|id| code(&label(string(id))))
            .collect();
        out.push_str(&format!(
            "## Retired operations\n\nThe release after this one dropped them, so this release and those before it no longer serve them: {}.\n\n",
            retired.join(", ")
        ));
    }
    out.push_str(&upgrading(d, name));
    let Some(from) = release["from"].as_str() else {
        return out;
    };
    out.push_str("## Changes\n\n");
    if list(&release["changes"]).is_empty() {
        out.push_str("No change a client of the release before it would notice.\n\n");
    }
    for change in list(&release["changes"]) {
        out.push_str(&change_section(d, change, from));
    }
    out
}

/// A release page's links to the upgrade guides that start from it.
fn upgrading(d: &Description, name: &str) -> String {
    let releases = list(&d.versions["releases"]);
    let guides: Vec<String> = upgrade::pairs(d)
        .into_iter()
        .filter(|(from, _)| string(&releases[*from]["name"]) == name)
        .map(|(_, to)| {
            let target = string(&releases[to]["name"]);
            link(
                &format!("to {target}"),
                &format!("../{}", upgrade::path(name, target)),
            )
        })
        .collect();
    if guides.is_empty() {
        return String::new();
    }
    format!(
        "## Upgrading\n\nWhat a client of this release meets when it moves to a newer one, and what was added since: {}.\n\n",
        guides.join(", ")
    )
}

/// A documentation object as an item's `documentation`, for [`docs`].
fn json_doc(documentation: &Value) -> Value {
    serde_json::json!({ "documentation": documentation })
}

/// One change: what it did, its documentation, and what a client of
/// `from`, the release before, sees instead.
fn change_section(d: &Description, change: &Value, from: &str) -> String {
    let clients = format!("Clients of {}", release_code(from));
    let older = &change["older"];
    let member = || {
        let id = format!("{}${}", string(&change["shape"]), string(&change["member"]));
        element(d, &Value::String(id))
    };
    let route = |method: &Value, uri: &Value| code(&format!("{} {}", string(method), string(uri)));
    let (title, instead) = match string(&change["kind"]) {
        "rename" => (
            format!("Renamed {}", member()),
            format!("{clients} call it {}.", code(string(&older["member"]))),
        ),
        "require" => (
            format!("Required {}", member()),
            if older["filled"] == "default" {
                format!(
                    "{clients} may leave it out: the service uses {}.",
                    // A string default is model text: inert in code, as a
                    // release name is.
                    release_code(&older["default"].to_string())
                )
            } else {
                format!("{clients} may leave it out: the service fills it in.")
            },
        ),
        "addEnumValue" => (
            format!(
                "Added {} to {}",
                code(string(&change["value"])),
                element(d, &change["shape"])
            ),
            format!(
                "{clients} receive {} instead.",
                code(string(&older["value"]))
            ),
        ),
        "addError" => {
            let members: Vec<String> = list(&older["members"])
                .iter()
                .map(|m| {
                    format!(
                        "{} from {}",
                        code(string(&m["member"])),
                        code(string(&m["from"]))
                    )
                })
                .collect();
            (
                format!(
                    "{} can return {}",
                    element(d, &change["operation"]),
                    element(d, &change["error"])
                ),
                format!(
                    "{clients} receive {} instead{}.",
                    element(d, &older["error"]),
                    if members.is_empty() {
                        String::new()
                    } else {
                        format!(", with {}", members.join(", "))
                    }
                ),
            )
        }
        "move" => (
            format!(
                "Moved {} to {}",
                element(d, &change["operation"]),
                route(&change["method"], &change["uri"])
            ),
            format!("{clients} call {}.", route(&older["method"], &older["uri"])),
        ),
        "reshape" => {
            let replaced: Vec<String> = list(&change["members"])
                .iter()
                .map(|m| code(string(m)))
                .collect();
            let definition = &older["shapes"][string(&older["shape"])];
            (
                format!("Reshaped {}", element(d, &change["shape"])),
                format!(
                    "{} became {}. {clients} use the older definition:\n\n{}",
                    replaced.join(", "),
                    code(string(&change["into"])),
                    members(d, list(&definition["members"]).iter().cloned(), None).trim_end()
                ),
            )
        }
        "retype" => (
            format!("Retyped {} to {}", member(), type_of(d, &change["target"])),
            format!(
                "{clients} use {}, converted {}.",
                if older["member"].is_null() {
                    "an older type".to_owned()
                } else {
                    target(d, &older["member"])
                },
                if change["by"] == "hook" {
                    "by the service's hook"
                } else {
                    "by a declared conversion"
                }
            ),
        ),
        "removeOperation" => (
            format!("Removed {}", code(&label(string(&change["operation"])))),
            format!(
                "{clients} can still call it: {}.",
                route(&older["operation"]["method"], &older["operation"]["uri"])
            ),
        ),
        "behavior" => (
            format!(
                "Changed how {} behaves: {}",
                element(d, &change["operation"]),
                code(string(&change["name"]))
            ),
            String::new(),
        ),
        kind => (format!("Changed ({})", code(kind)), String::new()),
    };
    let mut out = format!("### {title}\n\n");
    if !string(&change["documentation"]["html"]).is_empty() {
        out.push_str(&docs(change));
    }
    if !instead.is_empty() {
        out.push_str(&format!("{instead}\n\n"));
    }
    out
}
