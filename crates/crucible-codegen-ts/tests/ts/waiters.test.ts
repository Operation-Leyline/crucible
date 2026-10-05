import assert from "node:assert/strict";
import { test } from "node:test";
import { ServiceClient, ByteStream, ServiceError, WaiterError, WaiterTimeout } from "./generated-endpoints/src/index.js";
import { waitFor, type WaiterSpec } from "./generated-endpoints/src/waiters.js";
import { parseBehavior } from "./generated-endpoints/src/runtime.js";

test("generated waiter follows renamed model members and disambiguates normal operation names", async () => {
  let attempts = 0;
  const client = new ServiceClient({ region: "local", fetch: async () => { attempts++; return new Response(`{"wire-status":"${attempts === 1 ? "WAIT" : "READY"}"}`); } });
  assert.equal(typeof client.waitUntilReady, "function");
  const result = await client.waitUntilReady_2({ account: "customer" }, { maxWaitTimeMs: 200, minDelayMs: 0, maxDelayMs: 0 });
  assert.equal(result.outcome, "response");
  if (result.outcome === "response") assert.equal(result.output.status, "READY");
  assert.equal(result.attempts, 2);
});

test("error-type success has a typed absent response and unmatched transport errors keep their cause", async () => {
  const missing = new ServiceError("Missing", "gone", { status: 404, headers: new Headers() }, "client");
  const description: WaiterSpec = { minDelay: 0, maxDelay: 0, acceptors: [{ state: "success", matcher: { kind: "errorType", name: "example#Missing" } }] };
  const success = await waitFor("Gone", description, {}, async (): Promise<{ value: string }> => { throw missing; }, { maxWaitTimeMs: 100 });
  assert.equal(success.outcome, "error"); if (success.outcome === "error") assert.equal(success.error, missing);
  const network = new Error("connection closed");
  try { await waitFor("Failed", description, {}, async () => { throw network; }, { maxWaitTimeMs: 100 }); assert.ok(false); }
  catch (error) { assert.ok(error instanceof WaiterError); assert.equal(error.cause, network); assert.equal(error.attempts, 1); }
});

test("waiter inputOutput and timestamp expressions use model values and retain failure messages", async () => {
  const description = parseBehavior('{"minDelay":0,"maxDelay":0,"acceptors":[{"state":"failure","matcher":{"kind":"inputOutput","path":{"expression":{"kind":"compare","comparator":"equal","left":{"kind":"subexpression","left":{"kind":"field","name":"input"},"right":{"kind":"field","name":"at"}},"right":{"kind":"subexpression","left":{"kind":"field","name":"output"},"right":{"kind":"field","name":"at"}}}},"comparator":"booleanEquals","expected":"true"},"message":{"expression":{"kind":"field","name":"messages"}}}]}') as WaiterSpec;
  const at = new Date("2026-10-01T00:00:00.125Z");
  try { await waitFor("Equal", description, { at }, async () => ({ at: at.getTime() / 1000, messages: ["failed", "reason"] }), { maxWaitTimeMs: 100 }); assert.ok(false); }
  catch (error) { assert.ok(error instanceof WaiterError); assert.deepEqual(error.detail, ["failed", "reason"]); }
});

test("waiter deadline bounds hung calls and delays remain cancellable", async () => {
  const description: WaiterSpec = { minDelay: 1, maxDelay: 2, acceptors: [] };
  try { await waitFor("Hung", description, {}, () => new Promise(() => {}), { maxWaitTimeMs: 5 }); assert.ok(false); }
  catch (error) { assert.ok(error instanceof WaiterTimeout); assert.equal(error.attempts, 1); }
  let attempts = 0; const controller = new AbortController();
  await assert.rejects(waitFor("Cancelled", description, {}, async () => { attempts++; setTimeout(() => controller.abort(new Error("cancelled")), 1); return {}; }, { maxWaitTimeMs: 100, signal: controller.signal }), /cancelled/);
  assert.equal(attempts, 1);
});

test("allStringEquals requires a nonempty array and retrying waiters release streaming responses", async () => {
  let attempts = 0, cancelled = 0;
  const description: WaiterSpec = { minDelay: 0, maxDelay: 0, acceptors: [{ state: "success", matcher: { kind: "output", path: { expression: { kind: "field", name: "states" } }, comparator: "allStringEquals", expected: "READY" } }] };
  const result = await waitFor("Ready", description, {}, async () => { attempts++; return attempts === 1 ? { states: [], body: new ByteStream(new ReadableStream({ cancel() { cancelled++; } })) } : { states: ["READY"] }; }, { maxWaitTimeMs: 100 });
  assert.equal(result.attempts, 2); assert.equal(cancelled, 1);
});
