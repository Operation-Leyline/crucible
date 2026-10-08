import assert from "node:assert/strict";
import { test } from "node:test";
import { ApiClient, ApiException, PublicFault } from "./generated-audience/src/index.js";
import { ApiClient as InternalClient, InternalFault } from "./generated-audience-internal/src/index.js";

test("external client omits internal operations and uses unmodelled errors", async () => {
  for (const header of [true, false]) {
    const fetch = async () => new Response(JSON.stringify({ __type: "InternalFault", message: "private failure" }), {
      status: 400, headers: header ? { "X-Crucible-Error": "InternalFault" } : {},
    });
    const client = new ApiClient({ endpoint: "https://example.test", fetch, retry: { maxAttempts: 1 } });
    assert.equal("secret" in client, false);
    await assert.rejects(client.public({}), error => {
      assert(error instanceof ApiException);
      assert.equal(error instanceof PublicFault, false);
      assert.equal(error.name, "InternalFault");
      assert.equal(error.message, "private failure");
      assert.equal(error.$metadata.status, 400);
      return true;
    });
    const internal = new InternalClient({ endpoint: "https://example.test", fetch, retry: { maxAttempts: 1 } });
    assert.equal("secret" in internal, true);
    await assert.rejects(internal.public({}), error => error instanceof InternalFault);
  }
});
