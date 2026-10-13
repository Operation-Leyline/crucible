import assert from "node:assert/strict";
import { test } from "node:test";
import * as crucible from "./generated-payloadbindings/src/index.js";
import * as rest from "./generated-rest-payloadbindings/src/index.js";

for (const [sdk, protocol] of [[crucible, "Crucible JSON"], [rest, "REST JSON"]] as const) {
  test(`${protocol} document payload distinguishes omitted, null, and value`, async () => {
    const bodies: string[] = [];
    const client = new sdk.PayloadBindingsClient({endpoint: "https://example.test", fetch: async request => {
      bodies.push(await request.text());
      return new Response(null);
    }});
    await client.putDocument({});
    await client.putDocument({document: null});
    await client.putDocument({document: {answer: 42}});
    assert.deepEqual(bodies, ["", "null", '{"answer":42}']);
  });

  test(`${protocol} structured payload uses converted member presence`, async () => {
    const bodies: string[] = [];
    const client = new sdk.PayloadBindingsClient({endpoint: "https://example.test", fetch: async request => {
      bodies.push(await request.text());
      return new Response(null);
    }});
    await client.putRecord({});
    // JavaScript callers can supply null even when the typed input omits it.
    await client.putRecord({record: null} as unknown as crucible.RecordInput);
    await client.putRecord({record: {name: "record"}});
    assert.deepEqual(bodies, ["", protocol === "Crucible JSON" ? "null" : "", '{"name":"record"}']);
  });
}
