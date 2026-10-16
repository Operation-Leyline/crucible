use crucible_codegen_common::describe::Description;
use crucible_codegen_common::documentation;
use crucible_ir::ir::Service;
use serde_json::Value;

use crate::File;

/// What a generated sample is, as the static reference says it too.
const GENERATED_NOTE: &str = "Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).";

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
    if let Some(since) = n["since"].as_str() {
        out.push_str(&format!("Since {}.\n\n", code(since)));
    }
    if let Some(links) = n["externalDocs"].as_object() {
        for (name, url) in links {
            out.push_str(&documentation::html(&format!("[{name}]({})", string(url))));
        }
        if !links.is_empty() {
            out.push('\n');
        }
    }
    out
}
fn operation_path(item: &Value) -> String {
    format!("operations/API_{}.md", string(&item["name"]))
}
fn shape_path(item: &Value) -> String {
    format!("shapes/{}.md", string(&item["name"]))
}
fn link(label: &str, path: &str) -> String {
    format!("[{}]({path})", escaped(label))
}

pub(super) fn render(service: &Service, d: &Description) -> Vec<File> {
    let mut summary = "# Summary\n\n- [Service overview](index.md)\n".to_owned();
    let mut index = format!(
        "# {}\n\n{}{}Service version: {}.\n\n## Protocol\n\nWire protocol: {}.\n\n",
        heading(string(&d.service["title"])),
        notes(&d.service),
        docs(&d.service),
        code(string(&d.service["version"])),
        code(string(&d.service["protocol"]))
    );
    index.push_str("## Authentication\n\n");
    if list(&d.service["auth"]).is_empty() {
        index.push_str("No default authentication schemes.\n\n");
    }
    for scheme in list(&d.service["auth"]) {
        index.push_str(&format!(
            "- {}: {}\n",
            code(string(scheme)),
            code(&d.service["authSchemes"][string(scheme)].to_string())
        ));
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
    }
    files
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
        "| Error | HTTP status | Fault | Retryable | Description |\n|---|---|---|---|---|\n"
            .to_owned();
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
        out.push_str(&format!(
            "| {}{} | {} | {} | {retry} | {} |\n",
            link(
                string(&shape["name"]),
                &format!("{prefix}{}", shape_path(shape))
            ),
            badge(shape),
            e["status"],
            string(&e["fault"]),
            cell_docs(shape)
        ));
    }
    out.push('\n');
    out
}
fn cell_docs(item: &Value) -> String {
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
    if let Some(since) = item["notes"]["since"].as_str() {
        value.push_str(&format!(" Since {}.", code(since)));
    }
    if let Some(links) = item["notes"]["externalDocs"].as_object() {
        for (name, url) in links {
            value.push_str(&documentation::html(&format!("[{name}]({})", string(url))));
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
        let mut detail = cell_docs(&m);
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
    let mut out = format!("# {}\n\n{}{}", string(&op["name"]), notes(op), docs(op));
    out.push_str(&format!("Protocol: {}.\n\n## Authentication\n\nEffective schemes: {}. Optional authentication: {}.\n\n",
        code(string(&op["protocol"])),code(&op["auth"].to_string()),op["optionalAuth"]));
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
        "# {}\n\n{}{}Type: {}.\n\n",
        string(&shape["name"]),
        notes(shape),
        docs(shape),
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
                cell_docs(value)
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
