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
const SAMPLES = golden("samples-external");
const LINKS = golden("links-internal");
const LINKS_EXTERNAL = golden("links-external");
const HISTORY = golden("history-internal");
const HISTORY_EXTERNAL = golden("history-external");
const VERSIONED = golden("versioned-external");
const DESCRIPTIONS = { WEATHER, WEATHER_EXTERNAL, CATALOG, PERMISSIONS, PERMISSIONS_EXTERNAL, SAMPLES, LINKS, LINKS_EXTERNAL, HISTORY, HISTORY_EXTERNAL, VERSIONED };

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
      // Without examples, the generated sample's request and response.
      const messages = op.examples.length
        ? op.examples.reduce((n, e) => n + ["input", "output", "error"].filter((k) => e[k] != null).length, 0)
        : 2;
      const bodyShapes = [op.request, op.response].filter((b) => !b.payload && b.body.length).length;
      assert.equal(copies.length, 2 + bodyShapes + messages + 2, `${id} copy buttons`);
      if (op.pagination) assert.ok(text.includes(op.pagination.inputToken), `${id} pagination`);
      if (op.idempotencyToken) assert.ok(text.includes(op.idempotencyToken), `${id} idempotency token`);
      for (const error of op.errors) assert.ok(text.includes(description.shapes[error].name), `${id} error ${error}`);
    }
    for (const id of Object.keys(description.shapes)) {
      if (id.startsWith("crucible.api#")) continue;
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
  const write = CATALOG.operations["example.docs#WriteItem"];
  assert.match(x.requestSyntax(write), /^POST \/WriteItem HTTP\/1\.1/);
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

test("navigation groups operations by resource, then service operations, data types and releases", () => {
  const groups = x.navigation(x.index(WEATHER));
  assert.deepEqual(groups.map((g) => g.label), ["City", "Service operations", "Data types", "Releases"]);
  assert.deepEqual(groups[0].entries.map((e) => e.item.name), ["CreateCity", "GetCity", "GetCityPhoto", "ListCities", "PutCityPhoto"]);
  assert.ok(!groups[2].entries.some((e) => e.id.startsWith("crucible.api#")));
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

test("an operation without examples shows its generated sample, labelled as generated", () => {
  const m = x.index(WEATHER);
  const id = "example.weather#Ping";
  const op = WEATHER.operations[id];
  assert.equal(op.examples.length, 0);
  const view = x.operationView(m, id, null);
  const text = textOf(view);
  assert.match(text, /Generated sample/);
  assert.match(text, /not written by the API's authors/);
  assert.match(text, /the service may still refuse a value/);
  assert.ok(text.includes(x.exampleMessage(op.sample.input, true)), "the sample request");
  const labels = elements(view, "button").map((b) => b.attrs["aria-label"]);
  assert.ok(labels.includes("Copy generated sample request") && labels.includes("Copy generated sample response"));
  // Its heading keeps the examples anchor, so a link to it still lands.
  assert.ok(elements(view, "section").some((s) => s.attrs.id === "cx-examples"));
  // An operation with examples shows them, and no sample.
  const city = textOf(x.operationView(m, "example.weather#GetCity", null));
  assert.match(city, /Examples/);
  assert.doesNotMatch(city, /Generated sample/);
  // A placeholder that misses its constraints is named.
  const fallback = { ...op, sample: { ...op.sample, fallbacks: [{ path: "input.code", shape: "x#Y$code", pattern: "^x$", reason: "r" }] } };
  const described = { ...WEATHER, operations: { ...WEATHER.operations, [id]: fallback } };
  assert.match(textOf(x.operationView(x.index(described), id, null)), /do not meet their constraints: input\.code/);
  // And from the generator: a pattern outside its subset.
  assert.match(textOf(x.operationView(x.index(SAMPLES), "example.samples#Check", null)), /do not meet their constraints: input\.bounded\./);
});

const PAYLOAD = `<img src=x onerror=alert(1)>"'&<script>alert(2)</script>`;
const SAFE_HTML = "<p>&lt;script&gt;alert(3)&lt;/script&gt; <code>&lt;img src=x onerror=alert(4)&gt;</code></p>\n";

// Replace every display string the way a hostile model could, keeping ids and
// references (map keys, `target`, `errors`...) intact so pages still resolve.
function poison(value, key = "") {
  const DISPLAY = new Set([
    "name", "wireName", "targetName", "title", "message", "since", "pattern", "version", "text", "markdown",
    "wireCode", "bodyDiscriminator", "discriminatorHeader", "mediaType", "idempotencyToken", "default", "wireDefault",
    "added", "changed", "missing", "from", "sunset", "current", "member", "members", "into",
  ]);
  if (Array.isArray(value)) return value.map((v) => poison(v, key));
  if (value && typeof value === "object") {
    const out = {};
    for (const [k, v] of Object.entries(value)) {
      if (k === "html") out[k] = v === "" ? "" : SAFE_HTML;
      // A sample's messages, and a fallback naming a hostile path.
      else if (k === "sample") out[k] = { ...everyString(v), fallbacks: [{ path: `${PAYLOAD}path`, shape: "x#Y$z", pattern: PAYLOAD, reason: PAYLOAD }] };
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
const HOSTILE_HISTORY = poison(HISTORY);
const KNOWN_TAGS = "div|span|a|h1|h2|h3|p|ul|li|nav|main|section|header|table|caption|thead|tbody|tr|th|td|code|pre|button|dl|dt|dd|details|summary|strong";

function allViews(description) {
  const m = x.index(description);
  const views = [x.overviewView(m), ...x.navigation(m).map((g) => x.navView(m, [g], { kind: "overview" }))];
  for (const id of Object.keys(description.operations)) views.push(x.operationView(m, id, null));
  for (const id of Object.keys(description.shapes)) views.push(x.shapeView(m, id));
  for (const id of Object.keys(description.resources)) views.push(x.resourceView(m, id));
  for (const release of m.releases) views.push(x.releaseView(m, release.name));
  return views;
}

test("XSS: model strings render as text, only documentation.html is markup", () => {
  const allowed = new Set([SAFE_HTML]);
  let hostileTexts = 0;
  for (const view of [...allViews(HOSTILE), ...allViews(HOSTILE_HISTORY)]) {
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
  // A fallback's path is model text too: shown, never markup.
  const ping = x.operationView(x.index(HOSTILE), "example.weather#Ping", null);
  assert.ok(textOf(ping).includes(`${PAYLOAD}path`), "the hostile fallback path, as text");
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
    "history-external",
    "history-internal",
    "links-external",
    "links-internal",
    "permissions-external",
    "permissions-internal",
    "samples-external",
    "versioned-external",
    "weather-external",
    "weather-internal",
  ].sort());
});

test("doc links become hash routes to the page for their id", () => {
  const m = x.index(LINKS_EXTERNAL);
  const overview = htmlOf(x.overviewView(m));
  const anchor = (route, id, label) => `<a href="${route}" data-crucible-ref="${id}">${label}</a>`;
  const opAnchor = (route, id, label) => `<a href="${route}" data-crucible-ref="${id}" data-crucible-operation="example.links#GetWidget">${label}</a>`;
  assert.ok(overview.includes(anchor("#/op/example.links%23GetWidget", "example.links#GetWidget", "the read call")), overview);
  assert.ok(overview.includes(anchor("#/shape/example.links%23Widget", "example.links#Widget", "Widget")), overview);
  // A member or enum value opens its type's page.
  assert.ok(overview.includes(anchor("#/shape/example.links%23Widget", "example.links#Widget$name", "Widget.name")), overview);
  assert.ok(overview.includes(anchor("#/shape/example.links%23Color", "example.links#Color$RED", "Color.RED")), overview);
  assert.ok(overview.includes(anchor("#/resource/example.links%23Gadget", "example.links#Gadget", "Gadget")), overview);
  assert.ok(overview.includes(anchor("#/", "example.links#Links", "Links")), overview);
  // Internal API is plain text in the external reference.
  assert.ok(overview.includes("Internal: Reveal and Secret."), overview);
  // An operation's input and output members open its request and response.
  const get = htmlOf(x.operationView(m, "example.links#GetWidget", null));
  assert.ok(get.includes(opAnchor("#/op/example.links%23GetWidget/request", "example.links#GetWidgetInput$widgetId", "GetWidget.widgetId")), get);
  assert.ok(get.includes(opAnchor("#/op/example.links%23GetWidget/response", "example.links#GetWidgetOutput$widget", "GetWidget.widget")), get);
  assert.equal(x.parseRoute("#/op/example.links%23GetWidget/request").section, "request");
  // Member docs and enum value docs link too.
  assert.ok(htmlOf(x.shapeView(m, "example.links#Color")).includes(anchor("#/shape/example.links%23Color", "example.links#Color$BLUE", "Color.BLUE")));
  // The internal reference links internal API.
  const internal = htmlOf(x.overviewView(x.index(LINKS)));
  assert.ok(internal.includes(anchor("#/op/example.links%23Reveal", "example.links#Reveal", "Reveal")), internal);
});

test("a doc link to an id the description lacks keeps no address", () => {
  const m = x.index(LINKS_EXTERNAL);
  assert.equal(x.refHref(m, "example.links#Secret"), null);
  assert.equal(x.refHref(m, "example.links#Secret$value"), null);
  const node = x.rich('<p><a data-crucible-ref="example.links#Secret">Secret</a></p>', m);
  assert.equal(node.html, '<p><a data-crucible-ref="example.links#Secret">Secret</a></p>');
  // Without the model, as an extension may call it, the html is unchanged.
  assert.equal(x.rich('<a data-crucible-ref="example.links#Widget">W</a>').html, '<a data-crucible-ref="example.links#Widget">W</a>');
  // The route is built from the encoded id, never raw markup.
  const hostile = x.rich('<a data-crucible-ref="example.links#Widget&quot;&gt;&lt;script&gt;">W</a>', m);
  assert.ok(!hostile.html.includes("<script>") && !hostile.html.includes('href="#'), hostile.html);
});

test("an operation member link opens the operation it names, though operations share the structure", () => {
  const description = structuredClone(LINKS_EXTERNAL);
  // A second operation with GetWidget's input: the link's operation decides.
  description.operations["example.links#AnotherGet"] = { ...description.operations["example.links#GetWidget"], id: "example.links#AnotherGet", name: "AnotherGet" };
  const m = x.index(description);
  assert.equal(x.refHref(m, "example.links#GetWidgetInput$widgetId", "example.links#GetWidget"), "#/op/example.links%23GetWidget/request");
  assert.equal(x.refHref(m, "example.links#GetWidgetInput$widgetId", "example.links#AnotherGet"), "#/op/example.links%23AnotherGet/request");
  const html = x.rich('<a data-crucible-ref="example.links#GetWidgetInput$widgetId" data-crucible-operation="example.links#GetWidget">w</a>', m).html;
  assert.equal(html, '<a href="#/op/example.links%23GetWidget/request" data-crucible-ref="example.links#GetWidgetInput$widgetId" data-crucible-operation="example.links#GetWidget">w</a>');
});

// ---- release history (decisions 77 and 78) -------------------------------

test("history badges date each element and link its release's changelog", () => {
  const m = x.index(HISTORY_EXTERNAL);
  const get = htmlOf(x.operationView(m, "example.history#GetItem", null));
  assert.ok(get.includes('<a class="cx-badge cx-changed" href="#/release/v3">Changed in v3</a>'), get);
  const size = x.shapeView(m, "example.history#Size");
  const sizeHtml = htmlOf(size);
  assert.ok(sizeHtml.includes('<a class="cx-badge cx-changed" href="#/release/v2">Changed in v2</a>'), sizeHtml);
  assert.ok(sizeHtml.includes('<a class="cx-badge cx-added" href="#/release/v3">Added in v3</a>'), sizeHtml);
  // A member a reshape hides from older clients says so.
  assert.ok(textOf(size).includes("Not in v1"), textOf(size));
  assert.ok(htmlOf(x.shapeView(m, "example.history#Kind")).includes(">Added in v2</a>"));
  // Something older than every supported release carries no badge.
  assert.ok(!htmlOf(x.shapeView(m, "example.history#Names")).includes("cx-added"));
  // A new error is dated where an operation lists it.
  assert.ok(htmlOf(x.operationView(m, "example.history#GetItem", null)).includes(">Added in v3</a>"));
});

test("@since is the fallback for an element older than every release", () => {
  const description = structuredClone(HISTORY_EXTERNAL);
  const op = description.operations["example.history#DeleteItem"];
  op.notes.since = "2019";
  const m = x.index(description);
  assert.ok(!textOf(x.operationView(m, "example.history#DeleteItem", null)).includes("Since"));
  op.history.added = null;
  assert.ok(textOf(x.operationView(x.index(description), "example.history#DeleteItem", null)).includes("Since"));
});

test("each release has a changelog of its changes and what older clients see", () => {
  const m = x.index(HISTORY_EXTERNAL);
  assert.deepEqual(m.releases.map((r) => r.name), ["v1", "v2", "v3"]);
  const v1 = textOf(x.releaseView(m, "v1"));
  assert.ok(v1.includes("The oldest supported release.") && v1.includes("2027-01-01"), v1);
  assert.ok(v1.includes("PingItems"), "retired operations");
  assert.ok(htmlOf(x.releaseView(m, "v1")).includes("v1 is served until the end of 2026."), "sunset documentation");
  const v2 = x.releaseView(m, "v2");
  const v2Text = textOf(v2);
  for (const expected of ["Renamed ", "GetItemOutput.title", " call it ", "name", "the service uses ", "20", "Added ", "HUGE", "LARGE", "Reshaped ", "width", "height", "area", "Retyped ", "by the service's hook.", "Changed how ", "ordering"]) {
    assert.ok(v2Text.includes(expected), `v2: ${expected}`);
  }
  // A member the change names links to the page that has it.
  assert.ok(htmlOf(v2).includes('<a href="#/op/example.history%23GetItem/response"><code>GetItemOutput.title</code></a>'), htmlOf(v2));
  const v3 = textOf(x.releaseView(m, "v3"));
  for (const expected of ["Moved ", "GET /things/{id}", "GET /items/{id}", "ItemGone", "ItemNotFound", "fills it in", "by a declared conversion.", "Removed ", "CountItems", "GET /count"]) {
    assert.ok(v3.includes(expected), `v3: ${expected}`);
  }
  // The overview lists the releases and the header that carries them.
  const overview = textOf(x.overviewView(m));
  assert.ok(overview.includes("Crucible-Version") && overview.includes("Current"), overview);
  assert.match(textOf(x.releaseView(m, "v9")), /No release v9\./);
});

test("the external reference leaves out the steps on internal API", () => {
  const internal = x.index(HISTORY);
  const external = x.index(HISTORY_EXTERNAL);
  const all = (m) => m.releases.map((r) => textOf(x.releaseView(m, r.name))).join("\n");
  assert.ok(all(internal).includes("Audit") && all(internal).includes("secret"));
  assert.ok(!all(external).includes("Audit") && !all(external).includes("secret"));
});

test("releases are routes, a sidebar group and searchable", () => {
  assert.deepEqual(x.parseRoute("#/release/v2"), { kind: "release", id: "v2", section: null });
  assert.equal(x.href("release", "2026-10-01+rc"), "#/release/2026-10-01%2Brc");
  const m = x.index(HISTORY_EXTERNAL);
  const groups = x.navigation(m);
  assert.equal(groups.at(-1).label, "Releases");
  assert.deepEqual(groups.at(-1).entries.map((e) => e.id), ["v1", "v2", "v3"]);
  assert.deepEqual(x.filterNavigation(groups, "v2").flatMap((g) => g.entries.map((e) => e.id)), ["v2"]);
  assert.equal(x.pageFor(m, x.parseRoute("#/release/v3"), null).title, "Release v3");
  // A description without releases has no such group, and no section on the overview.
  const plain = structuredClone(HISTORY_EXTERNAL);
  plain.versions = null;
  const pm = x.index(plain);
  assert.ok(!x.navigation(pm).some((g) => g.kind === "releases"));
  assert.ok(!textOf(x.overviewView(pm)).includes("Crucible-Version"));
});

test("the e2e versioned service's reference renders its releases", () => {
  const m = x.index(VERSIONED);
  assert.deepEqual(m.releases.map((r) => r.name), ["v1", "v2", "v3"]);
  const v3 = textOf(x.releaseView(m, "v3"));
  assert.ok(v3.includes("Reshaped ") && v3.includes("Removed ") && v3.includes("CountItems"), v3);
  assert.ok(textOf(x.releaseView(m, "v1")).includes("2027-01-01"));
});

test("the DOM boot path draws release pages", () => {
  const { win, doc, app } = createWindow({ description: HISTORY_EXTERNAL });
  x.start(app, HISTORY_EXTERNAL, { window: win });
  win.setHash("#/release/v2");
  assert.ok(app.textContent.includes("Reshaped"), app.textContent.slice(0, 200));
  assert.equal(doc.title, "Release v2 · Shop");
});
