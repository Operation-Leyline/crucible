import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { Transport, list, string } from "./generated/src/runtime.js";

interface Case { wire: string; values: string[]; encoded: string; }
const cases: Case[] = JSON.parse(readFileSync(new URL("../../../../../tests/conformance/header-lists.json", import.meta.url), "utf8"));

for (const [index, fixture] of cases.entries()) {
  test(`header list conformance: ${index}`, async () => {
    for (const wire of [fixture.wire, fixture.encoded]) {
      const transport = new Transport({ endpoint: "https://example.invalid", fetch: async request => {
        assert.equal(request.headers.get("x-items"), fixture.encoded);
        return new Response(null, { headers: { "x-items": wire } });
      } }, { middleware: [], bearer: false }, {});
      const output = await transport.send<Record<string, unknown>>({ name: "HeaderCase", method: "GET", path: [],
        headers: [["items", "x-items", list(string)]],
        output: { body: false, headers: [["items", "x-items", list(string)]] },
        errors: [], retrySafe: true }, { items: fixture.values });
      assert.deepEqual(output.items, fixture.values);
    }
  });
}
