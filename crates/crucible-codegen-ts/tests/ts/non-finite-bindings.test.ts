import assert from "node:assert/strict";
import { test } from "node:test";
import { FloatsClient } from "./generated-non-finite/src/index.js";
import { fromWire, number, toWire } from "./generated-non-finite/src/runtime.js";

const values = [NaN, Infinity, -Infinity, 1.25];
const texts = ["NaN", "Infinity", "-Infinity", "1.25"];

test("restJson1 sends non-finite floats in headers, query and labels", async () => {
  for (const [index, value] of values.entries()) {
    let calls = 0;
    const client = new FloatsClient({ endpoint: "https://example.test", fetch: async request => {
      calls++;
      const url = new URL(request.url);
      assert.equal(url.pathname, `/floats/${texts[index]}/${texts[index]}`);
      for (const header of ["X-HF", "X-HD"]) assert.equal(request.headers.get(header), texts[index]);
      for (const query of ["qf", "qd"]) assert.equal(url.searchParams.get(query), texts[index]);
      return new Response(null);
    }});
    await client.put({ lf: value, ld: value, hf: value, hd: value, qf: value, qd: value });
    assert.equal(calls, 1);
  }
});

test("restJson1 decodes non-finite float response headers", async () => {
  for (const [index, value] of values.entries()) {
    const client = new FloatsClient({ endpoint: "https://example.test", fetch: async () =>
      new Response(null, { headers: { "X-HF": texts[index], "X-OF": texts[index] } }) });
    const output = await client.put({ lf: 0, ld: 0 });
    assert.equal(output.hf, value);
    assert.equal(output.hd, value);
  }
});

test("float schemas without nonFinite continue rejecting non-finite values", () => {
  for (const [index, value] of values.slice(0, 3).entries()) {
    assert.throws(() => toWire(number, value), /expected finite number/);
    assert.throws(() => fromWire(number, texts[index]), /expected finite number/);
  }
  assert.throws(() => fromWire({ kind: "number", nonFinite: true }, "inf"), /expected finite number/);
});
