import assert from "node:assert/strict";
import { test } from "node:test";
// Every model name here is also a JavaScript global the generated code uses.
import { GlobalsClient, Object as ObjectError, Error as ServerError, Color } from "./generated-globals/src/index.js";
import type { Array, ColorMap, Date, Map, Partial, Promise, Record, Set, Uint8Array } from "./generated-globals/src/index.js";

test("models named after JavaScript globals compile and round-trip", async () => {
  const record: Record = { name: "r" };
  const partial: Partial = { value: 1 };
  const promise: Promise = { settled: true };
  const buffer: Uint8Array = { size: 2 };
  const array: Array = [record];
  const set: Set = ["a"];
  const date: Date = "2026-10-05";
  const colors: ColorMap = { [Color.RED]: partial };
  const map: Map = { key: record };
  const bytes = new globalThis.Uint8Array([1, 2, 3]);
  const when = new globalThis.Date("2026-10-05T00:00:00.000Z");
  const client = new GlobalsClient({ endpoint: "https://globals.test", fetch: async request => {
    const body = JSON.parse(await request.text());
    assert.equal(body.bytes, "AQID");
    return new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } });
  } });
  const output = await client.json({ record, partial, promise, bytes, array, set, date, when, colors, map, buffer });
  assert.deepEqual(output.record, record); assert.deepEqual(output.colors, colors); assert.deepEqual(output.map, map);
  assert.deepEqual(output.buffer, buffer); assert.deepEqual(output.array, array); assert.deepEqual(output.set, set);
  assert.equal(output.date, date); assert.equal(output.promise?.settled, true);
  assert.ok(output.bytes instanceof globalThis.Uint8Array); assert.deepEqual([...output.bytes], [1, 2, 3]);
  assert.ok(output.when instanceof globalThis.Date); assert.equal(output.when.getTime(), when.getTime());
});

test("errors named after JavaScript globals are thrown as their own classes", async () => {
  const client = new GlobalsClient({ endpoint: "https://globals.test", retry: false, fetch: async request => {
    const status = (await request.text()).includes("fail") ? 500 : 400;
    const body = status === 400 ? { __type: "Object", message: "bad", data: "AQI=", record: { name: "x" } } : { __type: "Error", message: "down" };
    return new Response(JSON.stringify(body), { status });
  } });
  const error = await client.json({}).catch((error: unknown) => error);
  assert.ok(error instanceof ObjectError);
  assert.equal(error.message, "bad"); assert.deepEqual(error.record, { name: "x" });
  assert.ok(error.data instanceof globalThis.Uint8Array); assert.deepEqual([...error.data], [1, 2]);
  const server = await client.json({ date: "fail" }).catch((error: unknown) => error);
  assert.ok(server instanceof ServerError); assert.equal(server.$fault, "server");
});

// Operations named like what schemas.ts imports (Schema, OperationSpec, list) must not merge with or shadow those imports.
test("operations named like schemas.ts imports compile and reach their routes", async () => {
  const paths: string[] = [];
  const client = new GlobalsClient({ endpoint: "https://globals.test", retry: false, fetch: async request => {
    const path = new URL(request.url).pathname;
    paths.push(path);
    return new Response(JSON.stringify({ name: path }), { headers: { "content-type": "application/json" } });
  } });
  assert.deepEqual(await client.schema(), { name: "/schema" });
  assert.deepEqual(await client.operationSpec(), { name: "/operation-spec" });
  assert.deepEqual(await client.list(), { name: "/list" });
  assert.deepEqual(paths, ["/schema", "/operation-spec", "/list"]);
});
