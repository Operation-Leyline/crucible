import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { ServiceError, Transport } from "./generated/src/runtime.js";
import type { ErrorSpec } from "./generated/src/runtime.js";

interface Case {
  name: string;
  safe: boolean;
  status: number;
  error: string | null;
  failures: number;
  maxAttempts: number;
  attempts: number;
  success: boolean;
}

const cases: Case[] = JSON.parse(readFileSync(new URL("../../../../../tests/conformance/retry.json", import.meta.url), "utf8"));
const errors: Record<string, ErrorSpec> = Object.fromEntries(
  ["Ordinary", "Transient", "Throttled"].map(name => [name, {
    status: name === "Throttled" ? 429 : 400,
    create: (_fields, metadata) => new ServiceError(name, name, metadata, "client",
      name === "Ordinary" ? undefined : { throttling: name === "Throttled" }),
  } satisfies ErrorSpec]),
);

for (const c of cases) {
  test(`retry conformance: ${c.name}`, async () => {
    let attempts = 0;
    const transport = new Transport({
      endpoint: "https://example.invalid",
      retry: { maxAttempts: c.maxAttempts, baseDelayMs: 0 },
      fetch: async () => {
        attempts++;
        return new Response(JSON.stringify(c.error === null ? {} : { __type: c.error }), {
          status: attempts <= c.failures ? c.status : 200,
        });
      },
    }, { bearer: false }, errors);
    let success = false;
    try {
      await transport.send({ name: "RetryCase", method: "POST", path: [],
        errors: Object.keys(errors), retrySafe: c.safe }, {});
      success = true;
    } catch (error) {
      assert.ok(error instanceof ServiceError, `unexpected failure: ${error}`);
    }
    assert.equal(attempts, c.attempts, c.name);
    assert.equal(success, c.success, c.name);
  });
}

test("named errors outside the operation stay unknown and preserve their name", async () => {
  for (const name of ["Throttled", "__proto__", "constructor"]) {
    let attempts = 0;
    const transport = new Transport({ endpoint: "https://example.invalid",
      retry: { maxAttempts: 3, baseDelayMs: 0 }, fetch: async () => {
        attempts++;
        return new Response(JSON.stringify({ __type: name }), { status: 429 });
      },
    }, { bearer: false }, errors);
    await transport.send({ name: "Other", method: "POST", path: [], errors: [], retrySafe: false }, {})
      .then(() => { throw new Error("expected an error"); }, error => {
        assert.ok(error instanceof ServiceError);
        assert.equal(error.name, name);
        assert.equal(error.$retryable, undefined);
      });
    assert.equal(attempts, 1);
  }
});
