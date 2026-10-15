import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { createWindow } from "../../../crucible-codegen-docs/tests/site/fake-dom.mjs";

const require = createRequire(import.meta.url);
const api = require("../../src/explorer/runtime/try-it.js");
const description = JSON.parse(readFileSync(new URL("../../../crucible-codegen-docs/tests/golden/weather-internal/describe.json", import.meta.url)));
const base = "https://localhost:8080/stage/";

const contracts = JSON.parse(readFileSync(new URL("../../../crucible-codegen-common/tests/golden/explorer/request-vectors.json", import.meta.url)));
for (const vector of contracts.cases) {
  test(vector.name, () => {
    const vectorDescription = contracts.descriptions[vector.description];
    const request = api.buildRequest(vectorDescription, vectorDescription.operations[vector.operation], vector.input, base);
    const url = new URL(request.url);
    assert.equal(request.method, vector.expected.method);
    assert.equal(url.pathname, `/stage${vector.expected.uri}`);
    // Query order is not significant, but repeated names and percent-encoding are.
    assert.deepEqual(url.search.slice(1).split("&").filter(Boolean).sort(), (vector.expected.queryParams || []).slice().sort());
    for (const [name, value] of Object.entries(vector.expected.headers || {})) assert.equal(request.headers.get(name), value, name);
    if (vector.expected.body !== undefined) {
      if (request.body instanceof Uint8Array) assert.equal(new TextDecoder().decode(request.body), vector.expected.body);
      else assert.deepEqual(JSON.parse(request.body || "{}"), JSON.parse(vector.expected.body || "{}"));
    }
  });
}

test("labels, query parameters and the method come from the lowered bindings", () => {
  const get = Object.values(description.operations).find((op) => op.name === "GetCity");
  const request = api.buildRequest(description, get, { cityId: "c-12345678", at: "2014-04-29T18:30:38Z" }, base);
  const url = new URL(request.url);
  assert.equal(request.method, "GET");
  assert.equal(url.pathname, "/stage/cities/c-12345678");
  assert.equal(url.searchParams.get("at"), "2014-04-29T18:30:38Z");
  const list = Object.values(description.operations).find((op) => op.name === "ListCities");
  const listed = new URL(api.buildRequest(description, list, { maxResults: 2, tags: ["a b", "/"] }, base).url);
  assert.deepEqual(listed.search.slice(1).split("&").sort(), ["maxResults=2", "tag=%2F", "tag=a%20b"]);
});

test("service base preserves router mounts and gateway stages", () => {
  assert.equal(api.serviceBase("http://localhost:8080/explorer/#/op/X", 1).href, "http://localhost:8080/");
  assert.equal(api.serviceBase("https://localhost/stage/dev/tools/index.html?q=1#/op/X", 2).href, "https://localhost/stage/");
  assert.equal(api.serviceBase("https://localhost/stage/explorer/", 1).href, "https://localhost/stage/");
});

const op = Object.values(description.operations).find((candidate) => candidate.name === "GetCity");
const first = { description, operation: op.id, input: { cityId: "c-12345678" } };
test("read-only, unsupported protocols, streams and browser-unsafe requests fail explicitly", () => {
  assert.throws(() => api.buildRequest(first.description, { ...op, readonly: false }, first.input, base, {}, { readOnly: true }), /read-only/);
  assert.throws(() => api.buildRequest(first.description, { ...op, protocol: "unsupported" }, first.input, base), /native client or curl/);
  assert.throws(() => api.buildRequest(first.description, { ...op, request: { ...op.request, eventStream: {} } }, first.input, base), /native client or curl/);
  assert.throws(() => api.buildRequest(first.description, op, { ...first.input, cityId: ".." }, base), /Dot path/);
  assert.throws(() => api.buildRequest(first.description, op, [], base), /JSON object/);
});

test("auth uses lowered API key placement, bearer and prefixes", () => {
  const description = structuredClone(first.description);
  description.service.authSchemes = { header: { kind: "apiKey", name: "x-dev-key", location: "header", scheme: "Token" }, query: { kind: "apiKey", name: "key", location: "query" } };
  const request = api.buildRequest(description, { ...op, auth: ["header", "query"] }, first.input, base, { bearer: "secret", apiKeys: { header: "header-value", query: "query/value" } });
  assert.equal(request.headers.get("authorization"), "Bearer secret");
  assert.equal(request.headers.get("x-dev-key"), "Token header-value");
  assert.equal(new URL(request.url).searchParams.get("key"), "query/value");
});

test("curl quoting survives apostrophes and binary payloads", () => {
  const request = api.buildRequest(first.description, op, first.input, base);
  assert.match(api.curl({ ...request, url: request.url + "?q=it's" }), /'\\''/);
  assert.match(api.curl({ ...request, body: new Uint8Array([1]) }), /--data-binary @payload.bin/);
});

test("modeled error names and pagination tokens use description metadata", () => {
  const get = Object.values(description.operations).find((op) => op.name === "GetCity");
  assert.equal(api.errorName(description, get, { status: 404, headers: new Headers({ "X-Crucible-Error": "NotFoundError" }) }, {}), "NotFoundError");
  assert.equal(api.errorName(description, get, { status: 200, headers: new Headers() }, {}), null);
  const list = Object.values(description.operations).find((op) => op.name === "ListCities");
  assert.deepEqual(api.nextInput(list, { nextToken: "page-2" }, { maxResults: 2 }, description), { maxResults: 2, nextToken: "page-2" });
  assert.equal(api.nextInput(list, {}, {}, description), null);
});

test("schema form and raw JSON mount through the existing renderer hook", () => {
  const { win } = createWindow();
  const container = win.document.createElement("div");
  container.ownerDocument = win.document;
  const description = structuredClone(first.description);
  const operation = description.operations[first.operation];
  operation.examples = [{ title: "Bound members and JSON body", inputValues: first.input }];
  const saved = {};
  const before = { location: globalThis.location, sessionStorage: globalThis.sessionStorage, CRUCIBLE_EXPLORER_CONFIG: globalThis.CRUCIBLE_EXPLORER_CONFIG };
  globalThis.location = { href: `${base}explorer/` };
  globalThis.sessionStorage = { getItem: (key) => saved[key] || null, setItem: (key, value) => { saved[key] = value; } };
  globalThis.CRUCIBLE_EXPLORER_CONFIG = { readOnly: true, prefixSegments: 1 };
  try {
    api.panel(operation, container, { description });
    assert.ok(container.querySelector("form"));
    assert.ok(container.querySelector("textarea"));
    assert.match(container.textContent, /Raw JSON/);
    assert.deepEqual(JSON.parse(container.querySelector(".cx-try-json").value), first.input);
  } finally { Object.assign(globalThis, before); }
});
