import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const api = createRequire(import.meta.url)("../../src/explorer/runtime/try-it.js");
const base = process.env.EXPLORER_VERSION_URL;

test("Try it succeeds against a service requiring its release header", { skip: !base }, async () => {
  const description = await (await fetch(`${base}/explorer/describe.json`)).json();
  const operation = Object.values(description.operations).find((op) => op.name === "Read");
  const request = api.buildRequest(description, operation, {}, `${base}/`);
  const response = await fetch(request.url, request);
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { value: "v1" });
});

test("Try it preserves sender requests while naming the current release", { skip: !base }, async () => {
  const description = await (await fetch(`${base}/explorer/describe.json`)).json();
  const operation = Object.values(description.operations).find((op) => op.name === "Notify");
  const request = api.buildRequest(description, operation, { signature: "signature" }, `${base}/`);
  assert.equal(request.headers.get(description.versions.carrier.name), description.versions.current);
  const response = await fetch(request.url, request);
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { value: "v1" });
});
