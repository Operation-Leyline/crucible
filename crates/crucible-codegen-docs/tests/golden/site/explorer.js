// Crucible API reference renderer: one classic script, no dependencies, no build.
//
// It reads a `crucible.describe/1` description and draws it. The same file is
// copied beside every generated `describe.json`, and the in-service Explorer
// loads it unchanged and adds panels through `registerOperationPanel`.
//
// Safety rule: every model string reaches the page as a text node (`h` turns
// string children into text). The only HTML ever inserted is a description's
// `documentation.html`, which the generator sanitised at build time; it goes
// through `rich`, the single place that touches `innerHTML`. Its doc links,
// `<a data-crucible-ref="<id>">` anchors with no address, get the hash route
// of the page for their id there.
//
// Layout: pure view functions return plain node objects (`h`/`rich`), so tests
// can render a description under Node without a DOM. `mount` turns those nodes
// into DOM; `start` wires routing, search and the extension hook around it.
(function (root) {
  "use strict";

  const FORMAT = "crucible.describe/1";

  // ---------------------------------------------------------------- nodes

  // Event handlers are the only attributes that run code; they are functions
  // passed in `on`, never strings. Names that could become script or inline
  // style are refused so a mistake cannot loosen a strict CSP.
  const FORBIDDEN_ATTRIBUTE = /^(on|style$|srcdoc$)/i;

  function h(tag, attrs, ...children) {
    const clean = {};
    for (const key of Object.keys(attrs || {})) {
      if (FORBIDDEN_ATTRIBUTE.test(key) && key !== "on") throw new Error(`attribute ${key} is not allowed`);
      clean[key] = attrs[key];
    }
    return { tag, attrs: clean, children: children.flat(Infinity).filter((c) => c != null && c !== false) };
  }

  // Sanitised documentation HTML from the description; empty means no element.
  // With the indexed description `m`, each doc link gets its page's address.
  function rich(html, m) {
    if (typeof html !== "string" || !html.trim()) return null;
    return { tag: "div", attrs: { class: "cx-doc" }, children: [], html: m ? linkRefs(m, html) : html };
  }

  // The generator writes exactly this opening tag for a doc link, its id
  // escaped as attribute text; an anchor whose id this description does not
  // have (an internal one in an external reference never reaches it) stays
  // without an address.
  const DOC_LINK = /<a data-crucible-ref="([^"]*)"( data-crucible-operation="([^"]*)")?>/g;
  const UNESCAPES = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"' };
  const unescape = (raw) => raw.replace(/&(amp|lt|gt|quot);/g, (e) => UNESCAPES[e]);

  function linkRefs(m, html) {
    return html.replace(DOC_LINK, (anchor, raw, rest, operation) => {
      const target = refHref(m, unescape(raw), operation == null ? null : unescape(operation));
      return target ? `<a href="${escapeText(target)}" data-crucible-ref="${raw}"${rest || ""}>` : anchor;
    });
  }

  function mount(node, doc) {
    if (typeof node === "string" || typeof node === "number") return doc.createTextNode(String(node));
    const el = doc.createElement(node.tag);
    for (const [key, value] of Object.entries(node.attrs)) {
      if (key === "on") {
        for (const [event, handler] of Object.entries(value)) el.addEventListener(event, handler);
      } else if (value === true) {
        el.setAttribute(key, "");
      } else if (value !== false && value != null) {
        el.setAttribute(key, String(value));
      }
    }
    if (node.html != null) el.innerHTML = node.html;
    for (const child of node.children) el.appendChild(mount(child, doc));
    return el;
  }

  const ESCAPES = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" };
  const escapeText = (s) => String(s).replace(/[&<>"]/g, (c) => ESCAPES[c]);

  // Serialisation for tests and for inspecting what a view would put on the page.
  function toHTML(node) {
    if (typeof node === "string" || typeof node === "number") return escapeText(node);
    const attrs = Object.entries(node.attrs)
      .filter(([key, v]) => key !== "on" && v !== false && v != null)
      .map(([key, v]) => (v === true ? ` ${key}` : ` ${key}="${escapeText(v)}"`))
      .join("");
    return `<${node.tag}${attrs}>${node.html != null ? node.html : node.children.map(toHTML).join("")}</${node.tag}>`;
  }

  // ---------------------------------------------------------------- model

  function index(description) {
    if (!description || description.format !== FORMAT) {
      throw new Error(`unsupported description format: ${description && description.format}`);
    }
    const versions = description.versions || null;
    return {
      d: description,
      service: description.service,
      operations: description.operations || {},
      shapes: description.shapes || {},
      resources: description.resources || {},
      versions,
      // Oldest first, current last; empty unless the chain has more than
      // the current release, which then has no changelog to show.
      releases: versions && list(versions.releases).length > 1 ? list(versions.releases) : [],
    };
  }

  // An operation's input or output JSON Schema, self-contained, whichever
  // way describe.json carries it (docs/docs.md, "Operation schemas"): the
  // root `schemas.mode` is "shared" or "omit", and absent when inline.
  // Inline it is returned as it is, shared as its root definition with the
  // definitions it reaches, and omitted it is `null`. `key` is "inputSchema" or
  // "outputSchema"; `description` is a description or an `index` of one.
  function operationSchema(description, op, key) {
    const shared = description && description.d ? description.d.schemas : description && description.schemas;
    const schema = op && op[key];
    // "omit", or a mode this renderer does not know: no schemas to show.
    if (!schema || typeof schema !== "object" || (shared && shared.mode !== "shared")) return null;
    const prefix = "#/$defs/";
    if (typeof schema.$ref !== "string" || !schema.$ref.startsWith(prefix) || !shared || !shared.$defs) return schema;
    const defs = shared.$defs;
    const own = (name) => Object.prototype.hasOwnProperty.call(defs, name);
    const rootName = schema.$ref.slice(prefix.length);
    if (!own(rootName)) return schema;
    const reached = new Set();
    const pending = [rootName];
    const visit = (value) => {
      if (Array.isArray(value)) value.forEach(visit);
      else if (value && typeof value === "object") {
        for (const [k, v] of Object.entries(value)) {
          if (k === "$ref" && typeof v === "string" && v.startsWith(prefix)) pending.push(v.slice(prefix.length));
          else visit(v);
        }
      }
    };
    while (pending.length) {
      const name = pending.pop();
      if (reached.has(name) || !own(name)) continue;
      reached.add(name);
      visit(defs[name]);
    }
    const out = {};
    for (const [k, v] of Object.entries(defs[rootName])) Object.defineProperty(out, k, { value: v, enumerable: true, writable: true, configurable: true });
    const $defs = {};
    for (const name of [...reached].sort()) Object.defineProperty($defs, name, { value: defs[name], enumerable: true, writable: true, configurable: true });
    out.$defs = $defs;
    return out;
  }

  // Built-ins are `crucible.api#..`; a describe.json written before the
  // prelude moved spells them `smithy.api#..`.
  const isBuiltin = (id) => id.startsWith("crucible.api#") || id.startsWith("smithy.api#");
  const list = (v) => (Array.isArray(v) ? v : []);
  const names = (map) => Object.keys(map).sort();

  // Mirrors the Markdown reference: an operation appears under each resource
  // that binds it, unbound ones under "Service operations".
  function navigation(m) {
    const groups = [];
    for (const id of names(m.resources)) {
      const resource = m.resources[id];
      groups.push({
        kind: "resource",
        id,
        label: resource.name,
        item: resource,
        entries: list(resource.operations).filter((o) => m.operations[o]).map((o) => ({ kind: "op", id: o, item: m.operations[o] })),
      });
    }
    const unbound = names(m.operations).filter((id) => list(m.operations[id].resources).length === 0);
    if (unbound.length) {
      groups.push({ kind: "service", id: "", label: "Service operations", item: null, entries: unbound.map((o) => ({ kind: "op", id: o, item: m.operations[o] })) });
    }
    const shapes = names(m.shapes).filter((id) => !isBuiltin(id));
    if (shapes.length) {
      groups.push({ kind: "types", id: "", label: "Data types", item: null, entries: shapes.map((s) => ({ kind: "shape", id: s, item: m.shapes[s] })) });
    }
    if (m.releases.length) {
      groups.push({ kind: "releases", id: "", label: "Releases", item: null, entries: m.releases.map((r) => ({ kind: "release", id: r.name, item: r })) });
    }
    return groups;
  }

  function searchText(entry) {
    const item = entry.item;
    const parts = [item.name, entry.id];
    if (entry.kind === "op") parts.push(item.method, item.uri);
    if (entry.kind === "shape") parts.push(item.kind);
    return parts.join(" ").toLowerCase();
  }

  // Every query term must appear; a group stays when its own name matches.
  function filterNavigation(groups, query) {
    const terms = String(query || "").toLowerCase().split(/\s+/).filter(Boolean);
    if (!terms.length) return groups;
    const hit = (text) => terms.every((t) => text.includes(t));
    const out = [];
    for (const group of groups) {
      const all = hit(group.label.toLowerCase());
      const entries = all ? group.entries : group.entries.filter((e) => hit(searchText(e)));
      if (entries.length) out.push({ ...group, entries });
    }
    return out;
  }

  // ---------------------------------------------------------------- routes

  const enc = encodeURIComponent;

  function href(kind, id, section) {
    if (kind === "overview") return "#/";
    return `#/${kind}/${enc(id)}${section ? `/${enc(section)}` : ""}`;
  }

  // The page a doc link's id names: the service overview, an operation,
  // resource or data type, a member or enum value on its type's page, and an
  // operation input's or output's member in that operation's request or
  // response. `null` when the description has none.
  // `operation`, from `data-crucible-operation`, is the operation an
  // `[Operation.member]` link names, exact where operations share a shape.
  function refHref(m, id, operation) {
    if (id === m.service.id) return href("overview");
    if (m.operations[id]) return href("op", id);
    if (m.resources[id]) return href("resource", id);
    const at = id.indexOf("$");
    if (at < 0) return m.shapes[id] && !isBuiltin(id) ? href("shape", id) : null;
    const shape = id.slice(0, at);
    const named = operation && m.operations[operation];
    if (named && named.input === shape) return href("op", operation, "request");
    if (named && named.output === shape) return href("op", operation, "response");
    for (const opId of names(m.operations)) {
      const op = m.operations[opId];
      if (op.input === shape) return href("op", opId, "request");
      if (op.output === shape) return href("op", opId, "response");
    }
    return m.shapes[shape] && !isBuiltin(shape) ? href("shape", shape) : null;
  }

  function parseRoute(hash) {
    const parts = String(hash || "").replace(/^#\/?/, "").split("/").filter(Boolean);
    if (!parts.length) return { kind: "overview" };
    const kind = parts[0];
    if (!["op", "shape", "resource", "release"].includes(kind) || !parts[1]) return { kind: "missing", raw: String(hash) };
    try {
      return { kind, id: decodeURIComponent(parts[1]), section: parts[2] ? decodeURIComponent(parts[2]) : null };
    } catch {
      return { kind: "missing", raw: String(hash) };
    }
  }

  // ---------------------------------------------------------------- pieces

  const text = (v) => (typeof v === "string" ? v : "");
  const code = (v) => h("code", {}, v);
  const link = (kind, id, label, extra) => h("a", { href: href(kind, id), class: extra }, label);

  // Only web links from model data become anchors; anything else stays text.
  function externalLink(label, url) {
    return /^(https?:\/\/|mailto:)/i.test(url) ? h("a", { href: url, rel: "noopener noreferrer", class: "cx-external" }, label) : h("span", {}, `${label}: ${url}`);
  }

  function badges(item) {
    const out = [];
    const n = item.notes || {};
    if (item.internal) out.push(h("span", { class: "cx-badge cx-internal" }, "Internal"));
    if (n.deprecated) {
      const d = n.deprecated;
      const detail = [d.since ? `since ${d.since}` : "", d.message || ""].filter(Boolean).join(": ");
      out.push(h("span", { class: "cx-badge cx-deprecated", title: detail || null }, "Deprecated"));
    }
    if (n.unstable) out.push(h("span", { class: "cx-badge cx-unstable" }, "Unstable"));
    if (item.sensitive) out.push(h("span", { class: "cx-badge cx-sensitive" }, "Sensitive"));
    return out;
  }

  // When the element arrived and changed (decision 77), each linked to its
  // release's changelog, and the releases that cannot see it.
  function historyBadges(item) {
    const hist = item && item.history;
    if (!hist) return [];
    const out = [];
    if (typeof hist.added === "string") out.push(h("a", { class: "cx-badge cx-added", href: href("release", hist.added) }, `Added in ${hist.added}`));
    for (const r of list(hist.changed)) out.push(h("a", { class: "cx-badge cx-changed", href: href("release", r) }, `Changed in ${r}`));
    const missing = list(hist.missing).map(text);
    if (missing.length) out.push(h("span", { class: "cx-badge cx-missing", title: `Clients of ${missing.join(", ")} use an older definition without it.` }, `Not in ${missing.join(", ")}`));
    return out;
  }

  function noteBlock(item) {
    const n = item.notes || {};
    const out = [];
    if (item.internal) out.push(h("p", { class: "cx-note cx-internal" }, h("strong", {}, "Internal API. "), "Not part of the external contract."));
    if (n.deprecated) {
      const d = n.deprecated;
      out.push(h("p", { class: "cx-note cx-deprecated" }, h("strong", {}, "Deprecated"), d.since ? ` since ${d.since}` : "", d.message ? `: ${d.message}` : "."));
    }
    if (n.unstable) out.push(h("p", { class: "cx-note cx-unstable" }, h("strong", {}, "Unstable API. "), "This API may change."));
    // `@since` is the author's fallback for what predates every release.
    if (n.since && !(item.history && item.history.added)) out.push(h("p", { class: "cx-muted" }, "Since ", code(n.since), "."));
    const links = Object.keys(n.externalDocs || {});
    if (links.length) out.push(h("p", { class: "cx-links" }, links.map((name) => externalLink(name, n.externalDocs[name]))));
    return out;
  }

  // Prose from the description (already HTML) plus the structured notes.
  function prose(m, item) {
    return [noteBlock(item), rich(item.documentation && item.documentation.html, m)];
  }

  function pretty(value) {
    return JSON.stringify(value, null, 2);
  }

  // A block of text with a copy button; the button's handler is attached by
  // `mount` through `on`, so the view stays data.
  function codeBlock(label, body, language) {
    return h(
      "div",
      { class: "cx-code" },
      h("div", { class: "cx-code-bar" }, h("span", {}, label), h("button", { type: "button", class: "cx-copy", "data-copy": body, "aria-label": `Copy ${label}` }, "Copy")),
      h("pre", { tabindex: "0", "data-language": language || null }, h("code", {}, body)),
    );
  }

  function table(columns, rows, caption) {
    return h(
      "div",
      { class: "cx-table" },
      h(
        "table",
        {},
        h("caption", { class: "cx-sr" }, caption),
        h("thead", {}, h("tr", {}, columns.map((c) => h("th", { scope: "col" }, c)))),
        h("tbody", {}, rows.map((row) => h("tr", {}, row.map((cell) => h("td", {}, cell))))),
      ),
    );
  }

  function constraintText(c) {
    if (!c) return "";
    const out = [];
    for (const key of ["length", "range"]) {
      const r = c[key];
      if (r) out.push(`${key} ${r.min == null ? "…" : r.min}–${r.max == null ? "…" : r.max}`);
    }
    if (c.pattern != null) out.push(`pattern ${c.pattern}`);
    if (c.uniqueItems) out.push("unique items");
    return out.join("; ");
  }

  function typeCell(m, member) {
    const target = m.shapes[member.target];
    if (target && !isBuiltin(member.target)) return link("shape", member.target, member.targetName);
    return code(member.targetType);
  }

  function describeMember(m, member) {
    return [badges(member), historyBadges(member), rich(member.documentation && member.documentation.html, m), noteBlock(member)];
  }

  function location(bindings, member) {
    const name = member.name;
    if (list(bindings.labels).includes(name)) return `path: ${name}`;
    for (const key of ["query", "headers"]) {
      const hit = list(bindings[key]).find((b) => b.member === name);
      if (hit) return `${key}: ${hit.name}`;
    }
    if (bindings.payload && bindings.payload.member === name) return "payload";
    return `body: ${member.wireName}`;
  }

  function memberTable(m, members, bindings, caption) {
    const rows = members.map((member) => [
      h("code", { class: member.required ? "cx-required" : null }, member.name),
      code(bindings ? location(bindings, member) : member.wireName),
      typeCell(m, member),
      member.required ? "Yes" : "No",
      member.default == null ? "—" : code(JSON.stringify(member.default)),
      constraintText(member.constraints) || "—",
      describeMember(m, member),
    ]);
    const columns = ["Member", bindings ? "Wire location" : "Wire name", "Type", "Required", "Default", "Constraints", "Description"];
    return table(columns, rows, caption);
  }

  // ---------------------------------------------------------------- syntax

  function requestSyntax(op) {
    const b = op.request;
    let uri = op.uri;
    for (const q of list(b.query)) uri += `${uri.includes("?") ? "&" : "?"}${q.name}={${q.member}}`;
    const lines = [`${op.method} ${uri} HTTP/1.1`];
    for (const [name, value] of Object.entries(op.protocolHeaders || {})) lines.push(`${name}: ${value}`);
    for (const header of list(b.headers)) lines.push(`${header.name}: {${header.member}}`);
    return finishSyntax(lines, b);
  }

  function responseSyntax(op) {
    const b = op.response;
    const lines = [`HTTP/1.1 ${op.status}`];
    for (const [name, value] of Object.entries(op.responseProtocolHeaders || {})) lines.push(`${name}: ${value}`);
    for (const header of list(b.headers)) lines.push(`${header.name}: {${header.member}}`);
    return finishSyntax(lines, b);
  }

  function finishSyntax(lines, b) {
    return b.payload ? `${lines.join("\n")}\n\n{${b.payload.member}}` : lines.join("\n");
  }

  function bodySkeleton(members) {
    const out = {};
    for (const member of members) out[member.wireName] = `<${member.targetType}>`;
    return pretty(out);
  }

  // The members of an operation's input/output, split the way the wire does.
  function splitMembers(m, shapeId, b) {
    const shape = shapeId && m.shapes[shapeId];
    const all = shape ? list(shape.members) : [];
    const inBody = (member) =>
      list(b.body).includes(member.name) || (b.payload && b.payload.member === member.name);
    return { shape, all, parameters: all.filter((x) => !inBody(x)), body: all.filter(inBody) };
  }

  function messageSection(m, op, request) {
    const title = request ? "Request" : "Response";
    const b = request ? op.request : op.response;
    const { shape, all, parameters, body } = splitMembers(m, request ? op.input : op.output, b);
    const out = [sectionHeading(op, request ? "request" : "response", `${title} syntax`)];
    out.push(codeBlock(`${title.toLowerCase()} syntax`, request ? requestSyntax(op) : responseSyntax(op), "http"));
    if (!shape) out.push(h("p", { class: "cx-muted" }, "No modeled body."));
    if (parameters.length) out.push(h("h3", {}, `${title} parameters`), memberTable(m, parameters, b, `${title} parameters`));
    if (body.length) {
      out.push(h("h3", {}, `${title} body`));
      if (!b.payload) out.push(codeBlock(`${title.toLowerCase()} body shape`, bodySkeleton(body), "json"));
      out.push(memberTable(m, body, b, `${title} body members`));
    }
    if (shape && !all.length) out.push(h("p", { class: "cx-muted" }, "No modeled members."));
    return h("section", { id: sectionId(request ? "request" : "response") }, out);
  }

  const sectionId = (name) => `cx-${name}`;

  // What the caller must hold (docs/permissions.md §6.3), as the Markdown
  // reference's Authorization section states it; nothing for an operation
  // that declares nothing.
  function authorizationSection(m, op) {
    const r = op.requirements;
    if (!r || !r.kind) return null;
    const body = [];
    if (r.kind === "none") body.push(h("p", {}, "Requires no permission."));
    else if (r.kind === "pending") body.push(h("p", {}, "Requirements not declared yet; the service checks them itself: ", r.reason));
    else if (r.kind === "internal") body.push(h("p", {}, "Requires permissions internal to the service."));
    else {
      // Markdown holding model text, rendered and sanitised at build time.
      for (const s of list(op.requirementDocumentation)) body.push(rich(s.html, m));
      const rows = list(r.clauses).map((c) => [
        c.mode,
        code(c.permission),
        code(c.resourceType),
        list(c.bindings).map((b, i) => [i ? ", " : "", code(b.identifier), " = ", code(b.value)]),
        c.when ? code(c.when) : "",
        // Model Markdown, rendered and sanitised at build time like the rest.
        (c.renderedDocumentation && rich(c.renderedDocumentation.html, m)) || c.documentation || "",
      ]);
      body.push(table(["Mode", "Permission", "Resource type", "Bindings", "When", "Notes"], rows, "Requirements"));
    }
    return h("section", { id: sectionId("authorization") }, sectionHeading(op, "authorization", "Authorization"), body);
  }

  // The permission registry by resource type (docs/permissions.md §6.3).
  function permissionsSection(s) {
    if (!list(s.permissions).length) return [];
    return [
      h("h2", {}, "Permissions"),
      h("p", {}, "What a grant names: a resource type and a permission value. A deprecated alias means the same as the value it names."),
      ...list(s.permissions).flatMap((r) => [
        h("h3", {}, code(r.resourceType)),
        table(
          ["Permission", "Tier", "Description"],
          list(r.permissions).map((p) => [
            code(p.value),
            p.tier,
            [p.description || "", p.aliasOf ? [" Deprecated alias of ", code(p.aliasOf), "."] : p.deprecated ? " Deprecated." : ""],
          ]),
          `Permissions of ${r.resourceType}`,
        ),
      ]),
    ];
  }

  function sectionHeading(op, section, title) {
    return h("h2", {}, title, " ", h("a", { class: "cx-anchor", href: href("op", op.id, section), "aria-label": `Link to ${title}` }, "#"));
  }

  // ---------------------------------------------------------------- errors

  function errorsTable(m, ids) {
    if (!list(ids).length) return h("p", { class: "cx-muted" }, "No modeled errors.");
    const rows = list(ids).map((id) => {
      const shape = m.shapes[id] || { name: id, error: {}, documentation: {}, notes: {} };
      const e = shape.error || {};
      const retry = !e.retryable ? "No" : e.retryable.throttling ? "Yes (throttling)" : "Yes";
      return [[link("shape", id, shape.name), badges(shape), historyBadges(shape)], String(e.status == null ? "" : e.status), text(e.fault), retry, [rich(shape.documentation && shape.documentation.html, m), noteBlock(shape)]];
    });
    return table(["Error", "HTTP status", "Fault", "Retryable", "Description"], rows, "Errors");
  }

  // -------------------------------------------------------------- examples

  function exampleMessage(message, request, status) {
    const lines = [request ? `${message.method} ${message.uri} HTTP/1.1` : `HTTP/1.1 ${status}`];
    for (const [name, value] of Object.entries(message.headers || {})) lines.push(`${name}: ${value}`);
    let out = lines.join("\n");
    if (message.body != null) out += `\n\n${pretty(message.body)}`;
    const p = message.payload;
    if (p != null) {
      out += "\n\n";
      if (p.encoding === "base64") out += `<binary payload, base64: ${p.value}>`;
      else if (p.kind === "string") out += text(p.value);
      else out += pretty(p.value);
    }
    return out;
  }

  // With no examples, the description's generated sample stands in, marked
  // as generated: the model implies it, nobody wrote it.
  function sampleSection(op) {
    const s = op.sample;
    if (!s || !s.input) return null;
    const out = [
      sectionHeading(op, "examples", "Generated sample"),
      h("p", { class: "cx-muted cx-generated" }, "This operation has no examples. Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist)."),
    ];
    const fallbacks = list(s.fallbacks).map((f) => text(f.path));
    if (fallbacks.length) out.push(h("p", { class: "cx-muted" }, `Placeholders that do not meet their constraints: ${fallbacks.join(", ")}.`));
    out.push(codeBlock("generated sample request", exampleMessage(s.input, true), "http"));
    if (s.output) out.push(codeBlock("generated sample response", exampleMessage(s.output, false, op.status), "http"));
    return h("section", { id: sectionId("examples") }, out);
  }

  function examplesSection(m, op) {
    const examples = list(op.examples);
    if (!examples.length) return sampleSection(op);
    const out = [sectionHeading(op, "examples", "Examples")];
    examples.forEach((e, i) => {
      out.push(h("h3", {}, e.title || `Example ${i + 1}`), rich(e.documentation && e.documentation.html, m));
      if (e.allowConstraintErrors) out.push(h("p", { class: "cx-muted" }, "This example intentionally permits constraint violations."));
      for (const [key, label] of [["input", "Request"], ["output", "Response"], ["error", "Error response"]]) {
        if (e[key] == null) continue;
        const message = key === "error" ? e.error.message : e[key];
        const status = key === "error" ? e.error.status : op.status;
        out.push(codeBlock(label.toLowerCase(), exampleMessage(message, key === "input", status), "http"));
      }
    });
    return h("section", { id: sectionId("examples") }, out);
  }

  // ----------------------------------------------------------------- pages

  function pageHeader(m, title, item, extra) {
    return h("header", { class: "cx-page-head" }, h("h1", { id: "cx-title", tabindex: "-1" }, title, " ", badges(item), extra, historyBadges(item)), prose(m, item));
  }

  function overviewView(m) {
    const s = m.service;
    const out = [pageHeader(m, s.title || s.name, s)];
    out.push(
      h(
        "dl",
        { class: "cx-facts" },
        h("dt", {}, "Version"), h("dd", {}, code(s.version)),
        h("dt", {}, "Protocol"), h("dd", {}, code(s.protocol)),
        h("dt", {}, "Declared protocols"), h("dd", {}, list(s.protocols).map((p) => code(p))),
      ),
    );
    out.push(h("h2", {}, "Authentication"));
    out.push(
      list(s.auth).length
        ? h("ul", {}, list(s.auth).map((id) => h("li", {}, code(id), " ", code(JSON.stringify((s.authSchemes || {})[id] || {})))))
        : h("p", { class: "cx-muted" }, "No default authentication schemes."),
    );
    out.push(...permissionsSection(s));
    out.push(h("h2", {}, "Service errors"), errorsTable(m, s.errors));
    out.push(h("h2", {}, "Operations"));
    for (const group of navigation(m).filter((g) => g.kind === "resource" || g.kind === "service")) {
      out.push(h("h3", {}, group.kind === "resource" ? link("resource", group.id, group.label) : group.label, " ", group.item ? badges(group.item) : null));
      out.push(h("ul", { class: "cx-list" }, group.entries.map((e) => h("li", {}, link("op", e.id, e.item.name), " ", badges(e.item), summaryLine(e.item)))));
    }
    if (m.releases.length) {
      const carrier = m.versions.carrier || {};
      out.push(
        h("h2", {}, "Releases"),
        h("p", {}, "The current release is ", code(m.versions.current), ". A client names the release it was built for in the ", code(carrier.name), " header."),
        h("ul", { class: "cx-list" }, m.releases.map((r) => h("li", {}, link("release", r.name, r.name), " ", releaseBadges(r), r.sunset ? h("span", { class: "cx-muted" }, " — supported until ", code(r.sunset)) : null))),
      );
    }
    return out;
  }

  // ------------------------------------------------------------ releases

  const releaseBadges = (r) => [r.current ? h("span", { class: "cx-badge cx-current" }, "Current") : null, r.sunset ? h("span", { class: "cx-badge cx-deprecated" }, "Sunset") : null];

  // An id as the model names it: `Shape`, `Shape.member`, `Operation`.
  const idLabel = (id) => text(id).slice(text(id).indexOf("#") + 1).replace("$", ".");

  // An element a change names, linked to its page when the description has one.
  function element(m, id) {
    const target = typeof id === "string" ? refHref(m, id, null) : null;
    return target ? h("a", { href: target }, code(idLabel(id))) : code(idLabel(id));
  }

  // A type by id, as a member table shows it.
  function typeRef(m, id) {
    const shape = m.shapes[id];
    if (shape && !isBuiltin(id)) return link("shape", id, shape.name);
    return code(shape ? shape.kind : idLabel(id));
  }

  // One change of a release (docs/docs.md, the `versions` block): what it
  // did, its documentation, and what a client of `from` sees instead.
  function changeView(m, c, from) {
    const o = c.older || {};
    const clients = ["Clients of ", code(from)];
    const member = () => element(m, `${text(c.shape)}$${text(c.member)}`);
    const route = (method, uri) => code(`${text(method)} ${text(uri)}`);
    let title;
    let instead = null;
    switch (c.kind) {
      case "rename":
        title = ["Renamed ", member()];
        instead = [clients, " call it ", code(o.member), "."];
        break;
      case "require":
        title = ["Required ", member()];
        instead = o.filled === "default" ? [clients, " may leave it out: the service uses ", code(JSON.stringify(o.default)), "."] : [clients, " may leave it out: the service fills it in."];
        break;
      case "addEnumValue":
        title = ["Added ", code(c.value), " to ", element(m, c.shape)];
        instead = [clients, " receive ", code(o.value), " instead."];
        break;
      case "addError":
        title = [element(m, c.operation), " can return ", element(m, c.error)];
        instead = [clients, " receive ", element(m, o.error), " instead", list(o.members).map((x, i) => [i ? ", " : ", with ", code(x.member), " from ", code(x.from)]), "."];
        break;
      case "move":
        title = ["Moved ", element(m, c.operation), " to ", route(c.method, c.uri)];
        instead = [clients, " call ", route(o.method, o.uri), "."];
        break;
      case "reshape": {
        const older = (o.shapes || {})[o.shape];
        title = ["Reshaped ", element(m, c.shape)];
        instead = [
          h("p", {}, list(c.members).map((x, i) => [i ? ", " : "", code(x)]), " became ", code(c.into), ". ", clients, " use the older definition:"),
          older ? memberTable(m, list(older.members), null, `${idLabel(o.shape)} before ${from}`) : null,
        ];
        break;
      }
      case "retype":
        title = ["Retyped ", member(), " to ", typeRef(m, c.target)];
        instead = [clients, " use ", o.member ? typeRef(m, o.member.target) : "an older type", ", converted ", c.by === "hook" ? "by the service's hook." : "by a declared conversion."];
        break;
      case "removeOperation":
        title = ["Removed ", code(idLabel(c.operation))];
        instead = o.operation ? [clients, " can still call it: ", route(o.operation.method, o.operation.uri), "."] : null;
        break;
      case "behavior":
        title = ["Changed how ", element(m, c.operation), " behaves: ", code(c.name)];
        break;
      default:
        title = ["Changed (", code(c.kind), ")"];
    }
    const older = instead && c.kind === "reshape" ? instead : instead ? h("p", {}, instead) : null;
    return h("section", { class: "cx-change" }, h("h3", {}, title), rich(c.documentation && c.documentation.html, m), older);
  }

  function releaseView(m, name) {
    const r = m.releases.find((x) => x.name === name);
    if (!r) return missingView(`No release ${name}.`);
    const out = [h("header", { class: "cx-page-head" }, h("h1", { id: "cx-title", tabindex: "-1" }, "Release ", r.name, " ", releaseBadges(r)))];
    out.push(
      h(
        "dl",
        { class: "cx-facts" },
        h("dt", {}, "Follows"), h("dd", {}, r.from ? link("release", r.from, r.from) : "The oldest supported release."),
        h("dt", {}, "Supported until"), h("dd", {}, r.sunset ? code(r.sunset) : "No sunset declared."),
      ),
      rich(r.documentation && r.documentation.html, m),
    );
    const sunset = rich(r.sunsetDocumentation && r.sunsetDocumentation.html, m);
    if (sunset) out.push(h("h2", {}, "Sunset"), sunset);
    if (list(r.retired).length) {
      out.push(h("h2", {}, "Retired operations"), h("p", {}, "The release after this one dropped them, so this release and those before it no longer serve them: ", list(r.retired).map((id, i) => [i ? ", " : "", code(idLabel(id))]), "."));
    }
    if (r.from) {
      out.push(h("h2", {}, "Changes"));
      const changes = list(r.changes);
      if (!changes.length) out.push(h("p", { class: "cx-muted" }, "No change a client of the release before it would notice."));
      for (const c of changes) out.push(changeView(m, c, r.from));
    }
    return out;
  }

  function summaryLine(item) {
    const t = item.documentation && item.documentation.text;
    return t ? h("span", { class: "cx-muted" }, " — ", t.split("\n")[0]) : null;
  }

  function resourceView(m, id) {
    const r = m.resources[id];
    if (!r) return missingView(`No resource ${id}.`);
    const out = [pageHeader(m, r.name, r)];
    const slots = ["create", "put", "read", "update", "delete", "list"].filter((slot) => r.lifecycle && m.operations[r.lifecycle[slot]]);
    if (slots.length) {
      out.push(h("h2", {}, "Lifecycle"), table(["Lifecycle", "Operation"], slots.map((slot) => [slot, [link("op", r.lifecycle[slot], m.operations[r.lifecycle[slot]].name), badges(m.operations[r.lifecycle[slot]])]]), "Resource lifecycle"));
    }
    const identifiers = Object.keys(r.identifiers || {});
    if (identifiers.length) out.push(h("h2", {}, "Identifiers"), table(["Name", "Type"], identifiers.map((k) => [code(k), shapeRef(m, r.identifiers[k])]), "Identifiers"));
    const properties = Object.keys(r.properties || {});
    if (properties.length) out.push(h("h2", {}, "Properties"), table(["Name", "Type"], properties.map((k) => [code(k), shapeRef(m, r.properties[k])]), "Properties"));
    out.push(h("h2", {}, "Operations"), h("ul", { class: "cx-list" }, list(r.operations).filter((o) => m.operations[o]).map((o) => h("li", {}, link("op", o, m.operations[o].name), " ", badges(m.operations[o])))));
    if (list(r.resources).length) out.push(h("h2", {}, "Child resources"), h("ul", { class: "cx-list" }, list(r.resources).filter((c) => m.resources[c]).map((c) => h("li", {}, link("resource", c, m.resources[c].name)))));
    return out;
  }

  function shapeRef(m, id) {
    return m.shapes[id] && !isBuiltin(id) ? link("shape", id, m.shapes[id].name) : code(id.replace(/^(crucible|smithy)\.api#/, ""));
  }

  // `panels` are the extension hook's containers: Explorer fills them in `start`.
  function operationView(m, id, panels) {
    const op = m.operations[id];
    if (!op) return missingView(`No operation ${id}.`);
    const out = [
      pageHeader(m, op.name, op, h("span", { class: "cx-method", "data-method": op.method }, op.method)),
      panels,
      h(
        "section",
        { id: sectionId("overview") },
        h("p", {}, "Protocol ", code(op.protocol), ". Authentication: ", list(op.auth).length ? list(op.auth).map((a) => code(a)) : "none", "."),
        // A third party calls it and signs each request (decision 81).
        op.sender
          ? h(
              "p",
              { class: "cx-sender" },
              `Called by ${op.sender.name}, not by clients of this API. It takes no credential: ${op.sender.name} signs each request, and the service verifies the signature in the `,
              code(op.sender.header || ""),
              " header.",
            )
          : null,
      ),
      authorizationSection(m, op),
      messageSection(m, op, true),
      messageSection(m, op, false),
      h("section", { id: sectionId("errors") }, sectionHeading(op, "errors", "Errors"), errorsTable(m, op.errors)),
      h(
        "section",
        { id: sectionId("idempotency") },
        sectionHeading(op, "idempotency", "Idempotency and retries"),
        h("p", {}, "Readonly: ", op.readonly ? "yes" : "no", ". Idempotent: ", op.idempotent ? "yes" : "no", "."),
        // The same note as crucible_codegen_common::docs::IDEMPOTENCY_TOKEN_NOTE.
        op.idempotencyToken
          ? h(
              "p",
              {},
              "Idempotency token: ",
              code(op.idempotencyToken),
              ". SDKs send this automatically. When calling the API directly, send a unique value for each intended operation, and the same value when retrying it; a service may refuse a request without one.",
            )
          : null,
      ),
    ];
    if (op.pagination) {
      const rows = ["inputToken", "outputToken", "items", "pageSize"].filter((k) => op.pagination[k] != null).map((k) => [k, code(op.pagination[k])]);
      out.push(h("section", { id: sectionId("pagination") }, sectionHeading(op, "pagination", "Pagination"), table(["Role", "Member path"], rows, "Pagination"), h("p", {}, "Follow the output token until absent or empty; reject repeated tokens.")));
    }
    out.push(examplesSection(m, op));
    // A description may omit the schemas, or share their definitions.
    const schemas = [
      ["inputSchema", "Input schema", "input schema"],
      ["outputSchema", "Output schema", "output schema"],
    ]
      .map(([key, title, label]) => [operationSchema(m, op, key), title, label])
      .filter(([schema]) => schema);
    if (schemas.length) {
      out.push(
        h(
          "section",
          { id: sectionId("schemas") },
          sectionHeading(op, "schemas", "JSON Schemas"),
          schemas.map(([schema, title, label]) => h("details", {}, h("summary", {}, title), codeBlock(label, pretty(schema), "json"))),
        ),
      );
    }
    return out;
  }

  function shapeView(m, id) {
    const s = m.shapes[id];
    if (!s) return missingView(`No data type ${id}.`);
    const out = [pageHeader(m, s.name, s, h("span", { class: "cx-kind" }, s.kind))];
    if (s.sensitive) out.push(h("p", { class: "cx-note cx-sensitive" }, h("strong", {}, "Sensitive: "), "avoid logging this value."));
    if (s.error) {
      const e = s.error;
      out.push(h("p", {}, "Error: ", code(e.wireCode), ` — HTTP ${e.status}, ${e.fault} fault`, e.retryable ? (e.retryable.throttling ? ", retryable (throttling)" : ", retryable") : "", "."));
    }
    if (list(s.members).length) {
      out.push(h("h2", {}, "Members"), memberTable(m, s.members, null, "Members"));
      if (s.kind === "union") out.push(h("p", { class: "cx-muted" }, "A union carries exactly one member."));
    }
    if (list(s.values).length) {
      out.push(h("h2", {}, "Values"), table(["Name", "Wire value", "Description"], s.values.map((v) => [code(v.name), code(JSON.stringify(v.value)), [badges(v), historyBadges(v), rich(v.documentation && v.documentation.html, m), noteBlock(v)]]), "Values"));
    }
    const c = constraintText(s.constraints);
    if (c) out.push(h("p", {}, "Constraints: ", c, "."));
    return out;
  }

  function missingView(message) {
    return [h("h1", { id: "cx-title", tabindex: "-1" }, "Not found"), h("p", {}, message), link("overview", "", "Back to the overview")];
  }

  function pageFor(m, route, panels) {
    switch (route.kind) {
      case "overview": return { title: text(m.service.title || m.service.name), view: overviewView(m) };
      case "op": return { title: (m.operations[route.id] || {}).name || "Not found", view: operationView(m, route.id, panels) };
      case "shape": return { title: (m.shapes[route.id] || {}).name || "Not found", view: shapeView(m, route.id) };
      case "resource": return { title: (m.resources[route.id] || {}).name || "Not found", view: resourceView(m, route.id) };
      case "release": return { title: m.releases.some((r) => r.name === route.id) ? `Release ${route.id}` : "Not found", view: releaseView(m, route.id) };
      default: return { title: "Not found", view: missingView(`Unknown address ${route.raw}.`) };
    }
  }

  function navView(m, groups, route) {
    const current = route.kind === "overview" ? "overview" : `${route.kind}/${route.id}`;
    const items = [h("li", {}, h("a", { href: "#/", class: "cx-nav-link", "aria-current": current === "overview" ? "page" : null }, "Overview"))];
    for (const group of groups) {
      items.push(
        h(
          "li",
          { class: "cx-nav-group" },
          h("div", { class: "cx-nav-heading" }, group.kind === "resource" ? link("resource", group.id, group.label, "cx-nav-link") : group.label),
          h(
            "ul",
            {},
            group.entries.map((e) => {
              const key = `${e.kind}/${e.id}`;
              return h("li", {}, h("a", { href: href(e.kind, e.id), class: "cx-nav-link", "aria-current": key === current ? "page" : null }, e.item.name, " ", badges(e.item).slice(0, 2), e.kind === "op" ? h("span", { class: "cx-method", "data-method": e.item.method }, e.item.method) : null));
            }),
          ),
        ),
      );
    }
    if (groups.length === 0) items.push(h("li", { class: "cx-muted cx-empty" }, "No matches."));
    return h("ul", { class: "cx-nav-list" }, items);
  }

  // --------------------------------------------------------------- the app

  // Extension hook. Each registered panel is called as
  // `panel(operation, container, context)` every time an operation page is
  // drawn: `operation` is the description's operation object, `container` an
  // empty element it may fill, and `context` carries `description`, `shapes`
  // (the description's shape map) and `route`. A throwing panel is reported in
  // its container instead of breaking the page.
  const operationPanels = [];
  let running = null;

  function registerOperationPanel(panel) {
    if (typeof panel !== "function") throw new TypeError("an operation panel is a function (operation, container, context)");
    operationPanels.push(panel);
    if (running) running.render();
    return () => {
      const at = operationPanels.indexOf(panel);
      if (at >= 0) operationPanels.splice(at, 1);
      if (running) running.render();
    };
  }

  function copyText(win, value) {
    const clipboard = win.navigator && win.navigator.clipboard;
    if (clipboard && clipboard.writeText) return clipboard.writeText(value);
    return new Promise((resolve, reject) => {
      const doc = win.document;
      const area = doc.createElement("textarea");
      area.value = value;
      area.className = "cx-offscreen";
      doc.body.appendChild(area);
      area.select();
      try {
        doc.execCommand("copy") ? resolve() : reject(new Error("copy refused"));
      } catch (e) {
        reject(e);
      } finally {
        area.remove();
      }
    });
  }

  function start(container, description, options) {
    const win = (options && options.window) || root;
    const doc = win.document;
    const m = index(description);
    let query = "";
    let timer = 0;

    const nav = doc.createElement("nav");
    nav.setAttribute("aria-label", "API reference");
    nav.className = "cx-nav";
    const search = doc.createElement("input");
    search.type = "search";
    search.className = "cx-search";
    search.placeholder = "Search ( / )";
    search.setAttribute("aria-label", "Search operations and data types");
    const navHost = doc.createElement("div");
    const main = doc.createElement("main");
    main.id = "cx-main";
    main.className = "cx-main";
    main.tabIndex = -1;
    const live = doc.createElement("div");
    live.className = "cx-sr";
    live.setAttribute("aria-live", "polite");

    const brand = doc.createElement("a");
    brand.href = "#/";
    brand.className = "cx-brand";
    brand.textContent = text(m.service.title || m.service.name);
    const skip = doc.createElement("a");
    skip.href = "#cx-main";
    skip.className = "cx-skip";
    skip.textContent = "Skip to content";
    skip.addEventListener("click", (event) => {
      event.preventDefault();
      main.focus();
    });
    nav.append(brand, search, navHost);
    container.replaceChildren(skip, nav, main, live);

    const drawNav = () => navHost.replaceChildren(mount(navView(m, filterNavigation(navigation(m), query), parseRoute(win.location.hash)), doc));

    function render() {
      const route = parseRoute(win.location.hash);
      const panels = route.kind === "op" && m.operations[route.id] && operationPanels.length ? h("div", { class: "cx-panels", id: "cx-panels" }) : null;
      const page = pageFor(m, route, panels);
      main.replaceChildren(...page.view.flat(Infinity).filter(Boolean).map((n) => mount(n, doc)));
      doc.title = `${page.title} · ${text(m.service.title || m.service.name)}`;
      if (panels) {
        const host = main.querySelector("#cx-panels");
        for (const panel of operationPanels.slice()) {
          const slot = doc.createElement("div");
          slot.className = "cx-panel";
          host.appendChild(slot);
          try {
            panel(m.operations[route.id], slot, { description, shapes: m.shapes, route });
          } catch (error) {
            slot.textContent = `Panel failed: ${error && error.message}`;
          }
        }
      }
      drawNav();
      const target = route.section && doc.getElementById(sectionId(route.section));
      if (target) target.scrollIntoView();
      else if (win.scrollTo) win.scrollTo(0, 0);
      return route;
    }

    let rendered = null;
    const navigate = () => {
      // A plain fragment (a documentation link such as `#limits`) is an in-page
      // target, not a route: keep the current view instead of "Not found".
      const hash = String(win.location.hash || "");
      if (rendered && hash && !hash.startsWith("#/")) {
        let id = hash.slice(1);
        try {
          id = decodeURIComponent(id);
        } catch {
          /* keep the raw fragment */
        }
        const anchor = doc.getElementById(id);
        if (anchor) anchor.scrollIntoView();
        return;
      }
      const previous = rendered;
      rendered = render();
      // Move focus with the page so keyboard and screen-reader users land on
      // the new content, but not while they are typing in the search box.
      if (!previous || previous.kind !== rendered.kind || previous.id !== rendered.id) {
        if (previous && doc.activeElement !== search) main.focus({ preventScroll: true });
      }
    };

    win.addEventListener("hashchange", navigate);
    search.addEventListener("input", () => {
      query = search.value;
      drawNav();
      const groups = filterNavigation(navigation(m), query);
      live.textContent = `${groups.reduce((n, g) => n + g.entries.length, 0)} results`;
    });
    search.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        search.value = query = "";
        drawNav();
      } else if (event.key === "Enter") {
        const first = navHost.querySelector(".cx-nav-group ul a");
        if (first) first.click();
      } else if (event.key === "ArrowDown") {
        const first = navHost.querySelector(".cx-nav-link");
        if (first) {
          event.preventDefault();
          first.focus();
        }
      }
    });
    doc.addEventListener("keydown", (event) => {
      const t = event.target;
      const typing = t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
      if (event.key === "/" && !typing && !event.metaKey && !event.ctrlKey) {
        event.preventDefault();
        search.focus();
      }
    });
    // One delegated handler serves every copy button, however often pages redraw.
    main.addEventListener("click", (event) => {
      const button = event.target.closest && event.target.closest("button.cx-copy");
      if (!button) return;
      copyText(win, button.getAttribute("data-copy")).then(
        () => flash(button, "Copied"),
        () => flash(button, "Copy failed"),
      );
    });
    function flash(button, label) {
      button.textContent = label;
      live.textContent = label;
      win.clearTimeout(timer);
      timer = win.setTimeout(() => (button.textContent = "Copy"), 1500);
    }

    running = { render: navigate, container };
    navigate();
    return { render: navigate, description };
  }

  // Resolve the description: embedded script first (works from file://), then
  // a sibling describe.json.
  async function load(win) {
    if (win.CRUCIBLE_DESCRIPTION) return win.CRUCIBLE_DESCRIPTION;
    const response = await win.fetch("describe.json");
    if (!response.ok) throw new Error(`describe.json: HTTP ${response.status}`);
    return response.json();
  }

  async function boot(win) {
    const container = win.document.querySelector("[data-crucible-explorer]");
    if (!container || container.hasAttribute("data-manual")) return null;
    try {
      return start(container, await load(win), { window: win });
    } catch (error) {
      container.textContent = `Cannot load the API description: ${error && error.message}`;
      return null;
    }
  }

  const api = {
    FORMAT,
    h, rich, mount, toHTML,
    index, navigation, filterNavigation, parseRoute, href, refHref,
    overviewView, operationView, shapeView, resourceView, releaseView, pageFor, navView,
    requestSyntax, responseSyntax, exampleMessage, operationSchema,
    operationPanels, registerOperationPanel,
    start, load, boot,
  };

  if (typeof module !== "undefined" && module.exports) {
    module.exports = api;
  } else {
    root.CrucibleExplorer = api;
    if (root.document) {
      if (root.document.readyState === "loading") root.document.addEventListener("DOMContentLoaded", () => boot(root));
      else boot(root);
    }
  }
})(globalThis);
