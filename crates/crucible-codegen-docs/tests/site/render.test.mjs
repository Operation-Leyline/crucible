// Renderer tests: `node --test crates/crucible-codegen-docs/tests/site`.
// No npm packages. The goldens are the generator's own describe.json output, so
// a contract change that the renderer cannot handle fails here.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { createWindow } from "./fake-dom.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const crate = join(here, "..", "..");
const x = createRequire(import.meta.url)(join(crate, "runtime", "explorer.js"));
const golden = (name) => JSON.parse(readFileSync(join(crate, "tests", "golden", name, "describe.json"), "utf8"));

const WEATHER = golden("weather-internal");
const WEATHER_EXTERNAL = golden("weather-external");
const CATALOG = golden("catalog-internal");
const PERMISSIONS = golden("permissions-internal");
const PERMISSIONS_EXTERNAL = golden("permissions-external");
const DESCRIPTIONS = { WEATHER, WEATHER_EXTERNAL, CATALOG, PERMISSIONS, PERMISSIONS_EXTERNAL };

// ---- walking rendered nodes ----------------------------------------------

function walk(node, visit) {
  visit(node);
  if (typeof node !== "object") return;
  for (const child of node.children) walk(child, visit);
}
const nodes = (view) => {
  const out = [];
  for (const root of [view].flat(Infinity).filter(Boolean)) walk(root, (n) => out.push(n));
  return out;
};
const textOf = (view) =>
  nodes(view)
    .filter((n) => typeof n === "string" || typeof n === "number")
    .join("\n");
const elements = (view, tag) => nodes(view).filter((n) => typeof n === "object" && (!tag || n.tag === tag));
const htmlOf = (view) => [view].flat(Infinity).filter(Boolean).map(x.toHTML).join("");

// ---- every page renders ---------------------------------------------------

for (const [name, description] of Object.entries(DESCRIPTIONS)) {
  test(`${name}: overview, every operation, shape and resource render`, () => {
    const m = x.index(description);
    const overview = x.overviewView(m);
    assert.match(textOf(overview), new RegExp(description.service.title));
    for (const id of Object.keys(description.operations)) {
      const op = description.operations[id];
      const view = x.operationView(m, id, null);
      const text = textOf(view);
      assert.ok(text.includes(op.name), `${id} title`);
      // Request in protocol form, as the Markdown reference prints it.
      assert.ok(text.includes(x.requestSyntax(op).split("\n")[0]), `${id} request line`);
      assert.ok(text.includes(`HTTP/1.1 ${op.status}`), `${id} response line`);
      // One copy button per example message plus the syntax and schema blocks.
      const copies = elements(view, "button").filter((b) => b.attrs["data-copy"] != null);
      const messages = op.examples.reduce((n, e) => n + ["input", "output", "error"].filter((k) => e[k] != null).length, 0);
      const bodyShapes = [op.request, op.response].filter((b) => !b.payload && !b.eventStream && b.body.length).length;
      assert.equal(copies.length, 2 + bodyShapes + messages + 2, `${id} copy buttons`);
      if (op.pagination) assert.ok(text.includes(op.pagination.inputToken), `${id} pagination`);
      if (op.idempotencyToken) assert.ok(text.includes(op.idempotencyToken), `${id} idempotency token`);
      for (const error of op.errors) assert.ok(text.includes(description.shapes[error].name), `${id} error ${error}`);
    }
    for (const id of Object.keys(description.shapes)) {
      if (id.startsWith("smithy.api#")) continue;
      const view = x.shapeView(m, id);
      assert.ok(textOf(view).includes(description.shapes[id].name), id);
      for (const value of description.shapes[id].values || []) assert.ok(textOf(view).includes(value.name), `${id} ${value.name}`);
      for (const member of description.shapes[id].members) assert.ok(textOf(view).includes(member.name), `${id}.${member.name}`);
    }
    for (const id of Object.keys(description.resources)) {
      assert.ok(textOf(x.resourceView(m, id)).includes(description.resources[id].name));
    }
  });
}

test("request syntax places labels, query, headers and the raw payload", () => {
  const op = WEATHER.operations["example.weather#PutCityPhoto"];
  assert.equal(
    x.requestSyntax(op),
    `${op.method} ${op.uri} HTTP/1.1\nContent-Type: image/jpeg\nX-Photo-Caption: {caption}\n\n{photo}`,
  );
  const get = WEATHER.operations["example.weather#GetCity"];
  assert.equal(x.requestSyntax(get), "GET /cities/{cityId}?at={at} HTTP/1.1");
  const rpc = CATALOG.operations["example.docs#WriteItem"];
  assert.match(x.requestSyntax(rpc), /^POST \/service\/Catalog\/operation\/WriteItem HTTP\/1\.1\nAccept: application\/json\nContent-Type: application\/json\nSmithy-Protocol: rpc-v2-json$/);
});

test("operation pages carry parameter and body tables with wire locations", () => {
  const m = x.index(WEATHER);
  const text = textOf(x.operationView(m, "example.weather#GetCity", null));
  assert.match(text, /path: cityId/);
  assert.match(text, /query: at/);
  assert.match(text, /body: city/);
  const photo = textOf(x.operationView(m, "example.weather#PutCityPhoto", null));
  assert.match(photo, /headers: X-Photo-Caption/);
  assert.match(photo, /\bpayload\b/);
});

test("raw blob payload examples show decoded bytes as base64 text", () => {
  const op = WEATHER.operations["example.weather#GetCityPhoto"];
  const example = op.examples.find((e) => e.output && e.output.payload);
  if (example) assert.match(x.exampleMessage(example.output, false, op.status), /<binary payload, base64: /);
});

test("a requirement clause's documentation renders as prose, not Markdown source", () => {
  const m = x.index(PERMISSIONS);
  const [id, op] = Object.entries(PERMISSIONS.operations).find(([, o]) =>
    ((o.requirements || {}).clauses || []).some((c) => c.documentation && c.documentation.includes("`")),
  );
  const clause = op.requirements.clauses.find((c) => c.documentation && c.documentation.includes("`"));
  const cells = elements(x.operationView(m, id, null), "td");
  // The Notes cell holds the build-time HTML, and never the raw backticks.
  assert.ok(cells.some((td) => nodes(td).some((n) => typeof n === "object" && n.html === clause.renderedDocumentation.html)), id);
  assert.ok(!cells.some((td) => nodes(td).includes(clause.documentation)), id);
});

// ---- badges ---------------------------------------------------------------

test("internal, deprecated and unstable badges follow the description", () => {
  const m = x.index(WEATHER);
  const classes = (view) => elements(view, "span").map((s) => s.attrs.class || "");
  const status = x.operationView(m, "example.weather#Status", null);
  assert.ok(classes(status).some((c) => c.includes("cx-internal")), "internal operation");
  assert.match(textOf(status), /Internal API/);
  const sky = x.shapeView(m, "example.weather#Sky");
  assert.ok(classes(sky).some((c) => c.includes("cx-deprecated")), "deprecated enum value");
  assert.match(textOf(sky), /Reported as CLEAR with a cloud cover reading/);
  // External output has no internal badges at all.
  const ext = x.index(WEATHER_EXTERNAL);
  for (const id of Object.keys(WEATHER_EXTERNAL.operations)) {
    assert.ok(!classes(x.operationView(ext, id, null)).some((c) => c.includes("cx-internal")), id);
  }
  // Unstable: flip a note and look for the badge.
  const unstable = structuredClone(WEATHER);
  unstable.operations["example.weather#GetCity"].notes.unstable = true;
  assert.match(textOf(x.operationView(x.index(unstable), "example.weather#GetCity", null)), /Unstable API/);
  assert.ok(classes(x.operationView(x.index(unstable), "example.weather#GetCity", null)).some((c) => c.includes("cx-unstable")));
});

// ---- navigation, search, routes -------------------------------------------

test("navigation groups operations by resource, then service operations, then data types", () => {
  const groups = x.navigation(x.index(WEATHER));
  assert.deepEqual(groups.map((g) => g.label), ["City", "Service operations", "Data types"]);
  assert.deepEqual(groups[0].entries.map((e) => e.item.name), ["CreateCity", "GetCity", "GetCityPhoto", "ListCities", "PutCityPhoto"]);
  assert.ok(!groups[2].entries.some((e) => e.id.startsWith("smithy.api#")));
});

test("search needs every term, keeps matching groups and finds by method or path", () => {
  const groups = x.navigation(x.index(WEATHER));
  const names = (q) => x.filterNavigation(groups, q).flatMap((g) => g.entries.map((e) => e.item.name));
  assert.deepEqual(
    names("photo").sort(),
    ["GetCityPhoto", "GetCityPhotoInput", "GetCityPhotoOutput", "Photo", "PhotoSummary", "PutCityPhoto", "PutCityPhotoInput", "PutCityPhotoOutput"],
  );
  assert.deepEqual(names("get /cities").sort(), ["GetCity", "GetCityPhoto", "ListCities"]);
  assert.deepEqual(names("zzzz-nothing"), []);
  assert.equal(x.filterNavigation(groups, "  ").length, groups.length);
  assert.ok(names("CITY").includes("GetCity"), "case-insensitive");
});

test("hash routes round-trip shape ids that contain # and reject malformed ones", () => {
  for (const id of ["example.weather#GetCity", "a b#c/d%e"]) {
    for (const kind of ["op", "shape", "resource"]) {
      assert.deepEqual(x.parseRoute(x.href(kind, id)), { kind, id, section: null });
    }
  }
  assert.deepEqual(x.parseRoute(x.href("op", "example.weather#GetCity", "errors")), { kind: "op", id: "example.weather#GetCity", section: "errors" });
  assert.deepEqual(x.parseRoute(""), { kind: "overview" });
  assert.deepEqual(x.parseRoute("#/"), { kind: "overview" });
  assert.equal(x.parseRoute("#/op").kind, "missing");
  assert.equal(x.parseRoute("#/op/%E0%A4%A").kind, "missing");
  assert.equal(x.parseRoute("#/bogus/1").kind, "missing");
  const m = x.index(WEATHER);
  assert.match(textOf(x.pageFor(m, { kind: "op", id: "nope#Nope" }).view), /No operation/);
});

test("an unknown description format is refused", () => {
  assert.throws(() => x.index({ format: "crucible.describe/2" }), /unsupported description format/);
});

test("anchors are built from encoded ids, never raw model text", () => {
  const m = x.index(WEATHER);
  for (const el of elements(x.overviewView(m), "a")) {
    assert.match(el.attrs.href, /^(#\/|https?:)/);
  }
});

// ---- model text is never markup -------------------------------------------

const PAYLOAD = `<img src=x onerror=alert(1)>"'&<script>alert(2)</script>`;
const SAFE_HTML = "<p>&lt;script&gt;alert(3)&lt;/script&gt; <code>&lt;img src=x onerror=alert(4)&gt;</code></p>\n";

// Replace every display string the way a hostile model could, keeping ids and
// references (map keys, `target`, `errors`...) intact so pages still resolve.
function poison(value, key = "") {
  const DISPLAY = new Set([
    "name", "wireName", "targetName", "title", "message", "since", "pattern", "version", "text", "markdown",
    "wireCode", "bodyDiscriminator", "discriminatorHeader", "mediaType", "idempotencyToken", "default", "wireDefault",
  ]);
  if (Array.isArray(value)) return value.map((v) => poison(v, key));
  if (value && typeof value === "object") {
    const out = {};
    for (const [k, v] of Object.entries(value)) {
      if (k === "html") out[k] = v === "" ? "" : SAFE_HTML;
      else if (k === "examples" || k === "headers" || k === "protocolHeaders" || k === "responseProtocolHeaders") out[k] = everyString(v);
      else if (k === "externalDocs") out[k] = { [PAYLOAD]: "javascript:alert(5)", "<b>docs</b>": "https://example.com/?a=<b>" };
      else if (k === "value" && typeof v === "string") out[k] = PAYLOAD;
      else out[k] = poison(v, k);
    }
    return out;
  }
  return typeof value === "string" && DISPLAY.has(key) ? `${PAYLOAD}${key}` : value;
}
// Text that appears in wire examples: bodies, header names and values, titles.
function everyString(value, key = "") {
  if (Array.isArray(value)) return value.map((v) => everyString(v, key));
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, everyString(v, k)]));
  if (typeof value !== "string") return value;
  if (key === "html") return value === "" ? "" : SAFE_HTML;
  return ["uri", "method", "shape", "kind", "encoding"].includes(key) ? value : `${PAYLOAD}${key}`;
}

const HOSTILE = poison(WEATHER);
const KNOWN_TAGS = "div|span|a|h1|h2|h3|p|ul|li|nav|main|section|header|table|caption|thead|tbody|tr|th|td|code|pre|button|dl|dt|dd|details|summary|strong";

function allViews(description) {
  const m = x.index(description);
  const views = [x.overviewView(m), ...x.navigation(m).map((g) => x.navView(m, [g], { kind: "overview" }))];
  for (const id of Object.keys(description.operations)) views.push(x.operationView(m, id, null));
  for (const id of Object.keys(description.shapes)) views.push(x.shapeView(m, id));
  for (const id of Object.keys(description.resources)) views.push(x.resourceView(m, id));
  return views;
}

test("XSS: model strings render as text, only documentation.html is markup", () => {
  const allowed = new Set([SAFE_HTML]);
  let hostileTexts = 0;
  for (const view of allViews(HOSTILE)) {
    const all = nodes(view);
    // The only markup entry point is a documentation html node, with exactly
    // the (pre-sanitised) string the description carried.
    for (const n of all) {
      if (typeof n !== "object") continue;
      if (n.html != null) {
        assert.ok(allowed.has(n.html), `unexpected html: ${n.html}`);
        assert.equal(n.attrs.class, "cx-doc");
      }
      for (const [k, v] of Object.entries(n.attrs)) {
        assert.ok(!/^on./i.test(k) && k !== "style", `attribute ${k}`);
        if (k === "href") assert.match(String(v), /^(#\/[^<>"' ]*|https:\/\/)/, `href ${v}`);
        assert.ok(!/^\s*javascript:/i.test(String(v)), `${k}=${v}`);
      }
    }
    if (textOf(view).includes("<img src=x onerror=alert(1)>")) hostileTexts += 1;
    // Serialised without documentation html, no model markup is a tag.
    const serial = [view].flat(Infinity).filter(Boolean).map((n) => x.toHTML(stripHtml(n))).join("");
    assert.ok(!/<img|<script|<b>|<svg|<iframe/i.test(serial), serial.slice(0, 200));
    assert.ok(!new RegExp(`<(?!/?(?:${KNOWN_TAGS})[\\s>/])`).test(serial), "a tag outside the renderer's own set");
  }
  assert.ok(hostileTexts > 20, `hostile text should be shown, as text, on many pages (${hostileTexts})`);
});

function stripHtml(node) {
  if (typeof node !== "object") return node;
  return { ...node, html: null, children: node.children.map(stripHtml) };
}

test("XSS: javascript: links and unknown schemes never become anchors", () => {
  const m = x.index(HOSTILE);
  const all = [x.overviewView(m), ...Object.keys(HOSTILE.operations).map((id) => x.operationView(m, id, null))];
  const anchors = all.flatMap((v) => elements(v, "a")).map((a) => a.attrs.href);
  assert.ok(anchors.every((h) => !/^javascript:/i.test(h)));
  // The unsafe link text survives as text so nothing is silently dropped.
  assert.ok(all.some((v) => textOf(v).includes("javascript:alert(5)")));
  // A safe https link is still a link.
  const svc = structuredClone(WEATHER);
  assert.ok(elements(x.overviewView(x.index(svc)), "a").some((a) => a.attrs.href === "https://weather.example/docs"));
});

test("XSS: the DOM boot path writes innerHTML only for documentation, and only its own html", () => {
  const { win, doc, app } = createWindow({ description: HOSTILE });
  x.start(app, HOSTILE, { window: win });
  for (const route of ["#/", "#/op/example.weather%23CreateCity", "#/shape/example.weather%23Sky", "#/resource/example.weather%23City"]) {
    win.setHash(route);
  }
  assert.ok(doc.htmlWrites.length > 0);
  for (const write of doc.htmlWrites) {
    assert.equal(write.element.className, "cx-doc");
    assert.equal(write.html, SAFE_HTML);
  }
  // Text nodes carry the hostile strings verbatim.
  assert.ok(app.textContent.includes("<img src=x onerror=alert(1)>"));
});

test("source hygiene: one innerHTML write, no eval-like calls, no remote or inline resources", () => {
  const js = readFileSync(join(crate, "runtime", "explorer.js"), "utf8");
  assert.equal(js.match(/\.innerHTML\b/g).length, 1);
  for (const banned of ["outerHTML", "insertAdjacentHTML", "document.write", "eval(", "new Function", "setAttribute(\"style\"", "javascript:", "XMLHttpRequest", "import("]) {
    assert.ok(!js.includes(banned), banned);
  }
  const css = readFileSync(join(crate, "runtime", "explorer.css"), "utf8");
  assert.ok(!/@import|url\(|@font-face/.test(css), "no external or font resources");
  const html = readFileSync(join(crate, "runtime", "index.html"), "utf8");
  assert.ok(!/<style|style=|\son\w+=|<script(?![^>]*\ssrc=)/i.test(html), "inline code");
  const size = [js, css, html].reduce((n, s) => n + Buffer.byteLength(s), 0);
  assert.ok(size < 100 * 1024, `renderer is ${size} bytes`);
});

test("the attribute guard refuses handlers and inline styles", () => {
  assert.throws(() => x.h("a", { onclick: "alert(1)" }), /not allowed/);
  assert.throws(() => x.h("div", { style: "color:red" }), /not allowed/);
  assert.doesNotThrow(() => x.h("button", { on: { click() {} } }));
});

// ---- the live app against the fake DOM ------------------------------------

test("start renders the overview, follows the hash and moves focus with the page", () => {
  const { win, doc, app } = createWindow({ description: WEATHER });
  x.start(app, WEATHER, { window: win });
  assert.match(doc.title, /^Weather · Weather$/);
  assert.match(app.textContent, /Service errors/);
  win.setHash("#/op/example.weather%23GetCity/errors");
  assert.match(doc.title, /^GetCity/);
  assert.equal(doc.scrolledTo.id, "cx-errors");
  assert.equal(doc.activeElement.id, "cx-main");
  const current = app.querySelectorAll("a").filter((a) => a.getAttribute("aria-current") === "page");
  assert.equal(current.length, 1);
  assert.equal(current[0].getAttribute("href"), "#/op/example.weather%23GetCity");
  win.setHash("#/shape/example.weather%23Sky");
  assert.match(doc.title, /^Sky/);
  win.setHash("#/nowhere");
  assert.match(app.textContent, /Not found/);
});

test("a plain documentation fragment keeps the current page", () => {
  const { win, doc, app } = createWindow({ description: WEATHER, hash: "#/op/example.weather%23GetCity" });
  x.start(app, WEATHER, { window: win });
  const title = doc.title;
  win.setHash("#limits");
  assert.equal(doc.title, title);
  assert.doesNotMatch(app.textContent, /Not found|Unknown address/);
  win.setHash("#/nowhere");
  assert.match(app.textContent, /Not found/);
});

test("search filters the sidebar, Escape clears it, Enter opens the first match", () => {
  const { win, doc, app } = createWindow({ description: WEATHER });
  x.start(app, WEATHER, { window: win });
  const search = app.querySelector(".cx-search");
  const links = () => app.querySelectorAll("a").filter((a) => a.className.includes("cx-nav-link")).length;
  const before = links();
  search.value = "ping";
  search.dispatch("input");
  assert.ok(links() < before && links() >= 1);
  assert.match(app.textContent, /Ping/);
  search.dispatch("keydown", { key: "Escape" });
  assert.equal(links(), before);
  search.value = "zzz";
  search.dispatch("input");
  assert.match(app.textContent, /No matches/);
  // "/" focuses the search box unless typing elsewhere.
  doc.dispatch("keydown", { key: "/", target: doc.body });
  assert.equal(doc.activeElement, search);
});

test("copy buttons copy the exact text they were rendered with", async () => {
  const { win, app } = createWindow({ description: WEATHER });
  x.start(app, WEATHER, { window: win });
  win.setHash("#/op/example.weather%23GetCity");
  const buttons = app.querySelectorAll("button").filter((b) => b.hasAttribute("data-copy"));
  assert.ok(buttons.length >= 4);
  buttons[0].dispatch("click");
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(win.navigator.clipboard.copied, [buttons[0].getAttribute("data-copy")]);
  assert.equal(buttons[0].textContent, "Copied");
});

test("boot falls back to describe.json when no script description is embedded", async () => {
  const { win, app } = createWindow({ description: undefined });
  let asked = null;
  win.fetch = async (url) => ((asked = url), { ok: true, json: async () => WEATHER });
  await x.boot(win);
  assert.equal(asked, "describe.json");
  assert.match(app.textContent, /Service errors/);
  const failing = createWindow({ description: undefined });
  failing.win.fetch = async () => ({ ok: false, status: 404 });
  await x.boot(failing.win);
  assert.match(failing.app.textContent, /Cannot load the API description: describe\.json: HTTP 404/);
  const manual = createWindow({ description: WEATHER });
  manual.app.setAttribute("data-manual", "");
  assert.equal(await x.boot(manual.win), null);
});

// ---- the extension hook ---------------------------------------------------

test("operation panels receive (operation, container, context) on operation pages only", () => {
  x.operationPanels.length = 0;
  const { win, app } = createWindow({ description: WEATHER });
  const calls = [];
  x.start(app, WEATHER, { window: win });
  const unregister = x.registerOperationPanel((operation, container, context) => {
    calls.push(operation.id);
    container.textContent = `try ${operation.name} with ${Object.keys(context.shapes).length} shapes`;
    assert.equal(context.route.kind, "op");
  });
  assert.deepEqual(calls, [], "not drawn on the overview");
  win.setHash("#/op/example.weather%23GetCity");
  assert.deepEqual(calls, ["example.weather#GetCity"]);
  assert.match(app.querySelector("#cx-panels").textContent, /try GetCity with \d+ shapes/);
  // Panels registered later still appear (the page redraws).
  x.registerOperationPanel((op, container) => (container.textContent = "second"));
  assert.match(app.querySelector("#cx-panels").textContent, /second/);
  win.setHash("#/shape/example.weather%23Sky");
  assert.equal(app.querySelector("#cx-panels"), null);
  unregister();
  win.setHash("#/op/example.weather%23GetCity");
  assert.ok(!app.textContent.includes("try GetCity"));
  x.operationPanels.length = 0;
});

test("a throwing panel is contained and reported; non-functions are refused", () => {
  x.operationPanels.length = 0;
  const { win, app } = createWindow({ description: WEATHER, hash: "#/op/example.weather%23GetCity" });
  x.start(app, WEATHER, { window: win });
  x.registerOperationPanel(() => {
    throw new Error("<b>boom</b>");
  });
  assert.match(app.querySelector("#cx-panels").textContent, /Panel failed: <b>boom<\/b>/);
  assert.match(app.textContent, /Request syntax/);
  assert.throws(() => x.registerOperationPanel("nope"), /function/);
  x.operationPanels.length = 0;
});

test("an operation states what the caller must hold, and the overview lists the permissions", () => {
  const m = x.index(PERMISSIONS);
  const query = textOf(x.operationView(m, "example.leyline#ExecuteQuery", null));
  assert.match(query, /Authorization/);
  for (const value of ["graph:read", "graph:write", "history:read", "target.history.historyId", "has target.history"]) {
    assert.ok(query.includes(value), value);
  }
  // A clause's own documentation is model Markdown, rendered in its Notes cell.
  assert.ok(htmlOf(x.operationView(m, "example.leyline#ExecuteQuery", null)).includes("<p>Only when the parsed query writes.</p>"));
  assert.ok(textOf(x.operationView(m, "example.leyline#GetUser", null)).includes("Requires no permission."));
  assert.match(textOf(x.operationView(m, "example.leyline#RestoreBackup", null)), /history module moves/);
  // Nothing declared, nothing said.
  assert.ok(!textOf(x.operationView(m, "example.leyline#Health", null)).includes("Authorization"));
  const overview = textOf(x.overviewView(m));
  assert.ok(overview.includes("organization:read"), "the server audience lists internal permissions");
  assert.ok(overview.includes("graph:view") && overview.includes("Deprecated alias of "));

  // The client audience names no internal permission.
  const external = x.index(PERMISSIONS_EXTERNAL);
  assert.match(textOf(x.operationView(external, "example.leyline#GetOrganization", null)), /Requires permissions internal to the service\./);
  assert.ok(!textOf(x.overviewView(external)).includes("organization:read"));
  assert.match(htmlOf(x.operationView(external, "example.leyline#CreateGraph", null)), /<code>graph:write<\/code>/);
});

test("the test fixtures cover all goldens the generator ships", () => {
  const dirs = readdirSync(join(crate, "tests", "golden")).filter((d) => d !== "site").sort();
  assert.deepEqual(dirs, [
    "catalog-external",
    "catalog-internal",
    "permissions-external",
    "permissions-internal",
    "weather-external",
    "weather-internal",
  ]);
});
