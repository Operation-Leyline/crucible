import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { NotesClient, type UpdateInput } from "./generated-clearable/src/index.js";
import type { CitySummary } from "./generated/src/index.js";
import { S_UpdateInput } from "./generated-clearable/src/schemas.js";
import { toWire, fromWire, parseJson, stringifyJson } from "./generated-clearable/src/runtime.js";

const clear: UpdateInput = { description: null };
const omit: UpdateInput = { description: undefined };
const set: UpdateInput = { description: "set" };
// @ts-expect-error ordinary optional inputs do not gain null in their type
const ordinary: UpdateInput = { other: null };
// @ts-expect-error optional output types are unchanged
const output: Pick<CitySummary, "updatedAt"> = { updatedAt: null };
void ordinary; void output;

test("toWire preserves a clear and omits undefined", () => {
  assert.deepEqual(toWire(S_UpdateInput, clear), { description: null });
  assert.deepEqual(toWire(S_UpdateInput, omit), {});
  assert.deepEqual(toWire(S_UpdateInput, set), { description: "set" });
});

test("generated requests send omitted, null and value separately", async () => {
  const sent: unknown[] = [];
  const client = new NotesClient({ endpoint: "https://notes.test", retry: false, fetch: async request => {
    sent.push(JSON.parse(await request.text()));
    return new Response(null, { status: 200 });
  } });
  for (const input of [omit, clear, set]) await client.update(input);
  assert.deepEqual(sent, [{}, { description: null }, { description: "set" }]);
});

const corpus = JSON.parse(readFileSync(new URL("../../../../../tests/conformance/clearable-serialization.json", import.meta.url), "utf8")) as {
  cases: { id: string; target: string; wire: string; expect: { json: string; states: Record<string, string> } }[];
};
test("clearable conformance covers all three states", () => assert.equal(corpus.cases.length, 3));
for (const row of corpus.cases) test(`clearable serialization: ${row.id}`, () => {
  assert.equal(row.target, "example.clearable#UpdateInput");
  const native = fromWire(S_UpdateInput, parseJson(row.wire)) as UpdateInput;
  assert.deepEqual(JSON.parse(stringifyJson(toWire(S_UpdateInput, native))), JSON.parse(row.expect.json));
  for (const [name, expected] of Object.entries(row.expect.states)) {
    const object = native as Record<string, unknown>;
    const state = !Object.hasOwn(object, name) ? "absent" : object[name] === null ? "null" : "value";
    assert.equal(state, expected);
  }
});
