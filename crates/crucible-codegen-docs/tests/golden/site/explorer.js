// Crucible API reference renderer: one classic script, no dependencies, no build.
//
// It reads a `crucible.describe/1` description and draws it. The same file is
// copied beside every generated `describe.json`, and the in-service Explorer
// loads it unchanged and adds panels through `registerOperationPanel`.
//
// Safety rule: every model string reaches the page as a text node (`h` turns
// string children into text). The only HTML ever inserted is a description's
// `documentation.html`, which the generator sanitised at build time; it goes
// through `rich`, the single place that touches `innerHTML`.
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
  function rich(html) {
    return typeof html === "string" && html.trim() ? { tag: "div", attrs: { class: "cx-doc" }, children: [], html } : null;
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
    return {
      d: description,
      service: description.service,
      operations: description.operations || {},
      shapes: description.shapes || {},
      resources: description.resources || {},
    };
  }

  const isBuiltin = (id) => id.startsWith("smithy.api#");
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

  function parseRoute(hash) {
    const parts = String(hash || "").replace(/^#\/?/, "").split("/").filter(Boolean);
    if (!parts.length) return { kind: "overview" };
    const kind = parts[0];
    if (!["op", "shape", "resource"].includes(kind) || !parts[1]) return { kind: "missing", raw: String(hash) };
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

  function noteBlock(item) {
    const n = item.notes || {};
    const out = [];
    if (item.internal) out.push(h("p", { class: "cx-note cx-internal" }, h("strong", {}, "Internal API. "), "Not part of the external contract."));
    if (n.deprecated) {
      const d = n.deprecated;
      out.push(h("p", { class: "cx-note cx-deprecated" }, h("strong", {}, "Deprecated"), d.since ? ` since ${d.since}` : "", d.message ? `: ${d.message}` : "."));
    }
    if (n.unstable) out.push(h("p", { class: "cx-note cx-unstable" }, h("strong", {}, "Unstable API. "), "This API may change."));
    if (n.since) out.push(h("p", { class: "cx-muted" }, "Since ", code(n.since), "."));
    const links = Object.keys(n.externalDocs || {});
    if (links.length) out.push(h("p", { class: "cx-links" }, links.map((name) => externalLink(name, n.externalDocs[name]))));
    return out;
  }

  // Prose from the description (already HTML) plus the structured notes.
  function prose(item) {
    return [noteBlock(item), rich(item.documentation && item.documentation.html)];
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

  function describeMember(member) {
    return [badges(member), rich(member.documentation && member.documentation.html), noteBlock(member)];
  }

  function location(bindings, member) {
    const name = member.name;
    if (list(bindings.labels).includes(name)) return `path: ${name}`;
    for (const key of ["query", "headers", "prefixHeaders"]) {
      const hit = list(bindings[key]).find((b) => b.member === name);
      if (hit) return `${key}: ${hit.name}`;
    }
    if (bindings.payload && bindings.payload.member === name) return "payload";
    if (bindings.eventStream && bindings.eventStream.member === name) return "event stream";
    if (bindings.queryParams === name) return "query parameters";
    if (bindings.responseCode === name) return "HTTP status";
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
      describeMember(member),
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
    for (const header of list(b.prefixHeaders)) lines.push(`${header.name}{key}: {${header.member}}`);
    return finishSyntax(lines, b);
  }

  function responseSyntax(op) {
    const b = op.response;
    const lines = [`HTTP/1.1 ${op.status}`];
    for (const [name, value] of Object.entries(op.responseProtocolHeaders || {})) lines.push(`${name}: ${value}`);
    for (const header of list(b.headers)) lines.push(`${header.name}: {${header.member}}`);
    for (const header of list(b.prefixHeaders)) lines.push(`${header.name}{key}: {${header.member}}`);
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
      list(b.body).includes(member.name) || (b.payload && b.payload.member === member.name) || (b.eventStream && b.eventStream.member === member.name);
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
      if (!b.payload && !b.eventStream) out.push(codeBlock(`${title.toLowerCase()} body shape`, bodySkeleton(body), "json"));
      out.push(memberTable(m, body, b, `${title} body members`));
    }
    if (shape && !all.length) out.push(h("p", { class: "cx-muted" }, "No modeled members."));
    if (b.eventStream) out.push(h("p", {}, "The body carries an event stream; consume events incrementally."));
    if (b.payload && b.payload.streaming) out.push(h("p", {}, "The payload is streaming."));
    if (b.payload && b.payload.requiresLength) out.push(h("p", {}, "The payload requires a Content-Length header."));
    return h("section", { id: sectionId(request ? "request" : "response") }, out);
  }

  const sectionId = (name) => `cx-${name}`;

  // A generated sentence (`requirementSentences`): Markdown whose only
  // markup is code spans.
  function sentence(text) {
    return String(text)
      .split("`")
      .map((part, i) => (i % 2 ? code(part) : part));
  }

  // What the caller must hold (docs/permissions.md §6.3), as the Markdown
  // reference's Authorization section states it; nothing for an operation
  // that declares nothing.
  function authorizationSection(op) {
    const r = op.requirements;
    if (!r || !r.kind) return null;
    const body = [];
    if (r.kind === "none") body.push(h("p", {}, "Requires no permission."));
    else if (r.kind === "pending") body.push(h("p", {}, "Requirements not declared yet; the service checks them itself: ", r.reason));
    else if (r.kind === "internal") body.push(h("p", {}, "Requires permissions internal to the service."));
    else {
      for (const s of list(op.requirementSentences)) body.push(h("p", {}, sentence(s)));
      const rows = list(r.clauses).map((c) => [
        c.mode,
        code(c.permission),
        code(c.resourceType),
        list(c.bindings).map((b, i) => [i ? ", " : "", code(b.identifier), " = ", code(b.value)]),
        c.when ? code(c.when) : c.mode === "handler" && c.documentation ? c.documentation : "",
      ]);
      body.push(table(["Mode", "Permission", "Resource type", "Bindings", "When"], rows, "Requirements"));
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
      return [[link("shape", id, shape.name), badges(shape)], String(e.status == null ? "" : e.status), text(e.fault), retry, [rich(shape.documentation && shape.documentation.html), noteBlock(shape)]];
    });
    return table(["Error", "HTTP status", "Fault", "Retryable", "Description"], rows, "Errors");
  }

  // -------------------------------------------------------------- examples

  function exampleMessage(message, request, status) {
    const lines = [request ? `${message.method} ${message.uri} HTTP/1.1` : `HTTP/1.1 ${message.responseCode == null ? status : message.responseCode}`];
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

  function examplesSection(op) {
    const examples = list(op.examples);
    if (!examples.length) return null;
    const out = [sectionHeading(op, "examples", "Examples")];
    examples.forEach((e, i) => {
      out.push(h("h3", {}, e.title || `Example ${i + 1}`), rich(e.documentation && e.documentation.html));
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

  function pageHeader(title, item, extra) {
    return h("header", { class: "cx-page-head" }, h("h1", { id: "cx-title", tabindex: "-1" }, title, " ", badges(item), extra), prose(item));
  }

  function overviewView(m) {
    const s = m.service;
    const out = [pageHeader(s.title || s.name, s)];
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
    if (list(s.scopes).length) out.push(h("p", {}, "Default scopes: ", list(s.scopes).map((sc) => code(sc))));
    out.push(...permissionsSection(s));
    out.push(h("h2", {}, "Service errors"), errorsTable(m, s.errors));
    out.push(h("h2", {}, "Operations"));
    for (const group of navigation(m).filter((g) => g.kind !== "types")) {
      out.push(h("h3", {}, group.kind === "resource" ? link("resource", group.id, group.label) : group.label, " ", group.item ? badges(group.item) : null));
      out.push(h("ul", { class: "cx-list" }, group.entries.map((e) => h("li", {}, link("op", e.id, e.item.name), " ", badges(e.item), summaryLine(e.item)))));
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
    const out = [pageHeader(r.name, r)];
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
    return m.shapes[id] && !isBuiltin(id) ? link("shape", id, m.shapes[id].name) : code(id.replace(/^smithy\.api#/, ""));
  }

  // `panels` are the extension hook's containers: Explorer fills them in `start`.
  function operationView(m, id, panels) {
    const op = m.operations[id];
    if (!op) return missingView(`No operation ${id}.`);
    const out = [
      pageHeader(op.name, op, h("span", { class: "cx-method", "data-method": op.method }, op.method)),
      panels,
      h("section", { id: sectionId("overview") }, h("p", {}, "Protocol ", code(op.protocol), ". Authentication: ", list(op.auth).length ? list(op.auth).map((a) => code(a)) : "none", op.optionalAuth ? " (optional)" : "", "."), list(op.scopes).length ? h("p", {}, "Required scopes: ", list(op.scopes).map((s) => code(s))) : null),
      authorizationSection(op),
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
    out.push(examplesSection(op));
    out.push(
      h(
        "section",
        { id: sectionId("schemas") },
        sectionHeading(op, "schemas", "JSON Schemas"),
        h("details", {}, h("summary", {}, "Input schema"), codeBlock("input schema", pretty(op.inputSchema), "json")),
        h("details", {}, h("summary", {}, "Output schema"), codeBlock("output schema", pretty(op.outputSchema), "json")),
      ),
    );
    return out;
  }

  function shapeView(m, id) {
    const s = m.shapes[id];
    if (!s) return missingView(`No data type ${id}.`);
    const out = [pageHeader(s.name, s, h("span", { class: "cx-kind" }, s.kind))];
    if (s.sensitive) out.push(h("p", { class: "cx-note cx-sensitive" }, h("strong", {}, "Sensitive: "), "avoid logging this value."));
    if (s.streaming) out.push(h("p", {}, h("strong", {}, "Streaming: "), "values are consumed incrementally."));
    if (s.error) {
      const e = s.error;
      out.push(h("p", {}, "Error: ", code(e.wireCode), ` — HTTP ${e.status}, ${e.fault} fault`, e.retryable ? (e.retryable.throttling ? ", retryable (throttling)" : ", retryable") : "", "."));
    }
    if (list(s.members).length) {
      out.push(h("h2", {}, "Members"), memberTable(m, s.members, null, "Members"));
      if (s.kind === "union") out.push(h("p", { class: "cx-muted" }, "A union carries exactly one member."));
    }
    if (list(s.values).length) {
      out.push(h("h2", {}, "Values"), table(["Name", "Wire value", "Description"], s.values.map((v) => [code(v.name), code(JSON.stringify(v.value)), [badges(v), rich(v.documentation && v.documentation.html), noteBlock(v)]]), "Values"));
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
    index, navigation, filterNavigation, parseRoute, href,
    overviewView, operationView, shapeView, resourceView, pageFor, navView,
    requestSyntax, responseSyntax, exampleMessage,
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
