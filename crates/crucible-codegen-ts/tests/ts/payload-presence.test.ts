import assert from "node:assert/strict";
import { test } from "node:test";
import * as crucible from "./generated-payloadbindings/src/index.js";

for (const [sdk, protocol] of [[crucible, "Crucible JSON"]] as const) {
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
    await client.putRecord({record: null} as unknown as crucible.PutRecordInput);
    await client.putRecord({record: {name: "record"}});
    assert.deepEqual(bodies, ["", "null", '{"name":"record"}']);
  });
}

const bytes = new Uint8Array([80, 78, 71]);
type PayloadCall = (fetch: crucible.Fetch, contentType?: string, headers?: Record<string, string>) => Promise<void>;
const payloadCalls: readonly [string, PayloadCall][] = [
  ["Crucible JSON", async (fetch, contentType, headers) => {
    const client = new crucible.PayloadBindingsClient({endpoint: "https://example.test", fetch, headers});
    await client.upload({data: bytes, contentType});
  }],
];

for (const [protocol, call] of payloadCalls) {
  test(`${protocol} renamed payload sends bytes and media type`, async () => {
    for (const contentType of [undefined, "image/png"]) {
      await call(async request => {
        assert.deepEqual(new Uint8Array(await request.arrayBuffer()), bytes);
        assert.equal(request.headers.get("content-type"), contentType ?? "application/octet-stream");
        return new Response(null);
      }, contentType);
    }
  });
}

for (const [protocol, call] of payloadCalls) {
  test(`${protocol} payload media type outranks a client Content-Type default`, async () => {
    // A modeled Content-Type member, then the payload's media type, then the client default.
    for (const [contentType, expected] of [[undefined, "application/octet-stream"], ["image/png", "image/png"]] as const) {
      await call(async request => {
        assert.equal(request.headers.get("content-type"), expected);
        return new Response(null);
      }, contentType, {"Content-Type": "text/plain"});
    }
  });
}

for (const [sdk, protocol] of [[crucible, "Crucible JSON"]] as const) {
  test(`${protocol} document payload keeps its media type over a client Content-Type default`, async () => {
    const types: (string | null)[] = [];
    const client = new sdk.PayloadBindingsClient({endpoint: "https://example.test", headers: {"Content-Type": "text/plain"}, fetch: async request => {
      types.push(request.headers.get("content-type"));
      return new Response(null);
    }});
    await client.putDocument({document: {answer: 42}});
    assert.deepEqual(types, ["application/json"]);
  });
}
