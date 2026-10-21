import assert from "node:assert/strict";
import { test } from "node:test";
import { StreamsClient, WorkFailed } from "./generated-streams/src/index.js";

declare const process: { env: Record<string, string | undefined> };
const endpoint = process.env.CRUCIBLE_STREAMING_URL;

test("generated TypeScript client consumes a generated Rust service over HTTP", { skip: !endpoint }, async () => {
  const client = new StreamsClient({ endpoint: endpoint! });
  const events = await client.watch();
  const values = [];
  for await (const value of events) values.push(value);
  assert.deepEqual(values, [{ ready: "first" }]);
  assert.equal(events.metadata.status, 200);
  assert.ok(events.metadata.requestId);

  const late = await client.publish({ topic: "typed" });
  assert.deepEqual((await late.next()).value, { ready: "first" });
  await assert.rejects(late.next(), error => error instanceof WorkFailed && error.message === "step failed");
  await assert.rejects(client.publish({ topic: "open-fail" }), error => error instanceof WorkFailed && error.message === "opening failed");

  // The producer never completes. A buffering HTTP path cannot pass this.
  const pending = await client.publish({ topic: "hold" }, { timeoutMs: 2000 });
  assert.deepEqual((await pending.next()).value, { ready: "first" });
  await pending.cancel();
  assert.deepEqual(await client.read(), { value: "unary" });
});
