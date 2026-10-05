import assert from "node:assert/strict";
import { test } from "node:test";
import { ServiceClient, RenamedMissing } from "./generated-endpoints/src/index.js";
import { parseEndpoint, resolveEndpoint, type EndpointRules } from "./generated-endpoints/src/endpoints.js";
import { query, type Expression } from "./generated-endpoints/src/query.js";
import { Decimal, parseBehavior, parseJson, stringifyJson } from "./generated-endpoints/src/runtime.js";

test("generated rules resolve builtins, host prefixes and headers before dispatch", async () => {
  let calls = 0;
  const client = new ServiceClient({ region: "local", fetch: async request => {
    calls++; assert.equal(request.url, "https://customer.local.example.test/resource"); assert.equal(request.headers.get("x-region"), "local");
    return new Response('{"wire-status":"READY"}');
  } });
  assert.equal((await client.read({ account: "customer" })).status, "READY");
  for (const account of ["a.b", "-bad", "x/y", "x@evil", "", "x".repeat(64)]) await assert.rejects(client.read({ account }), /host label/);
  assert.equal(calls, 1);
  const noPrefix = new ServiceClient({ region: "local", disableHostPrefix: true, fetch: async request => { assert.equal(new URL(request.url).hostname, "local.example.test"); return new Response('{}'); } });
  await noPrefix.read({ account: "ignored" });
  await assert.rejects(new ServiceClient({ region: "unknown" }).read({ account: "customer" }), /Unknown region unknown/);
});

test("endpoint parameter precedence retains false and validates required values", () => {
  const rules: EndpointRules = { version: "1.0", parameters: { Name: { kind: "string", required: true }, Flag: { kind: "boolean", required: true } }, rules: [{ conditions: [{ function: "isSet", arguments: [{ kind: "reference", name: "Flag" }], assign: null }], result: { kind: "endpoint", url: { kind: "string", parts: [{ kind: "literal", value: "https://example.test/base" }] }, headers: { name: [{ kind: "reference", name: "Name" }] }, properties: { flag: { kind: "reference", name: "Flag" } } } }] };
  const sources = { Name: [{ kind: "inputMember", member: "name" }, { kind: "client", name: "name" }, { kind: "default", value: "default" }], Flag: [{ kind: "builtIn", name: "Flag" }] } as const;
  assert.equal(resolveEndpoint(rules, sources, { name: "input" }, { name: "client" }, { Flag: false }).headers.name[0], "input");
  assert.equal(resolveEndpoint(rules, sources, {}, { name: "client" }, { Flag: false }).headers.name[0], "client");
  assert.equal(resolveEndpoint(rules, sources, {}, {}, { Flag: false }).properties.flag, false);
  assert.throws(() => resolveEndpoint(rules, sources, {}, {}, {}), /missing/);
  assert.throws(() => resolveEndpoint(rules, sources, {}, {}, { Flag: 0 }), /wrong type/);
  assert.equal(parseEndpoint("https://example.test:443/a")?.authority, "example.test");
  assert.equal(parseEndpoint("https://[::1]:8443")?.isIp, true);
  for (const endpoint of ["file:///path", "https://user:password@example.test", "https://example.test?q=x", "https://example.test#x", "https://example.test:99999", "https://evil\n.example"]) assert.equal(parseEndpoint(endpoint), null);
});

test("service-renamed error classes retain their modeled wire discriminator",async()=>{
  const client=new ServiceClient({region:"local",fetch:async()=>new Response('{"__type":"Missing","message":"gone"}',{status:404})});
  try {await client.read({account:"customer"});throw new Error("expected modeled error");}
  catch(error) {assert.ok(error instanceof RenamedMissing);assert.equal((error as RenamedMissing).name,"Missing");assert.equal((error as RenamedMissing).message,"gone");}
});

const literal = (value: unknown): Expression => ({ kind: "literal", value });
const fn = (name: string, ...values: unknown[]): unknown => query({ kind: "function", function: name, arguments: values.map(literal) }, {});

test("compiled JMESPath comparisons and numeric functions preserve decimal and bigint precision", () => {
  const expression = parseBehavior('{"kind":"compare","comparator":"greaterThan","left":{"kind":"literal","value":12345678901234567890.000000001},"right":{"kind":"literal","value":12345678901234567890}}') as Expression;
  assert.equal(query(expression, {}), true);
  assert.equal(query({ ...expression, comparator: "equal" } as Expression, {}), false);
  assert.equal(fn("contains", [9007199254740992n], 9007199254740993n), false);
  assert.equal(stringifyJson(fn("sum", [new Decimal("0.1"), new Decimal("0.2")])), "3e-1");
  assert.equal(new Decimal(String(fn("avg", [new Decimal("0.1"), new Decimal("0.3")]))).equals(new Decimal("0.2")), true);
  assert.equal(new Decimal(String(fn("floor", new Decimal("-0.1")))).equals(new Decimal("-1")), true);
  assert.equal(new Decimal(String(fn("ceil", new Decimal("-0.1")))).equals(new Decimal("0")), true);
  assert.equal(String(fn("max", [9007199254740992n, 9007199254740993n])), "9007199254740993");
  assert.equal(fn("type", new Decimal("0.1")), "number");
  assert.equal(fn("to_string", parseJson('{"n":12345678901234567890.001}')), '{"n":12345678901234567890.001}');
});

test("compiled JMESPath projections, references, filters, slices, and standard functions", () => {
  const field: Expression = { kind: "field", name: "name" };
  const values = [{ name: "b", ready: true }, { name: "a", ready: false }];
  const projection: Expression = { kind: "projection", left: { kind: "identity" }, right: { kind: "condition", predicate: { kind: "field", name: "ready" }, then: field } };
  assert.deepEqual(query(projection, values), ["b"]);
  assert.deepEqual(query({ kind: "function", function: "sort_by", arguments: [literal(values), { kind: "expressionReference", expression: field }] }, {}), [values[1], values[0]]);
  assert.deepEqual(query({ kind: "slice", start: null, stop: null, step: -1 }, [1, 2, 3]), [3, 2, 1]);
  assert.deepEqual(query({ kind: "flatten", expression: { kind: "identity" } }, [[1], [2, 3], 4]), [1, 2, 3, 4]);
  assert.deepEqual(fn("merge", { a: 1 }, { b: 2 }), { a: 1, b: 2 });
  assert.equal(fn("length", "a😀"), 2); assert.equal(fn("reverse", "a😀"), "😀a");
  assert.equal(fn("join", ":", ["a", "b"]), "a:b"); assert.equal(fn("not_null", null, false), false);
  assert.equal(fn("starts_with", "abc", "a"), true); assert.equal(fn("ends_with", "abc", "c"), true);
  assert.deepEqual(fn("keys", { a: 1 }), ["a"]); assert.deepEqual(fn("values", { a: 1 }), [1]);
  assert.deepEqual(fn("to_array", false), [false]); assert.equal(fn("to_number", "bad"), null);
  assert.throws(() => fn("sort", [1, "a"]), /uniformly/); assert.throws(() => fn("contains", "a", 1), /string/);
});
