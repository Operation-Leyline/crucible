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

const bytes = new Uint8Array([80, 78, 71]);
const stream = () => new ReadableStream<Uint8Array>({start(controller) { controller.enqueue(bytes); controller.close(); }});
type PayloadCall = (fetch: crucible.Fetch, streaming: boolean, contentType?: string) => Promise<void>;
const payloadCalls: readonly [string, PayloadCall][] = [
  ["Crucible JSON", async (fetch, streaming, contentType) => {
    const client = new crucible.PayloadBindingsClient({endpoint: "https://example.test", fetch});
    if (streaming) await client.uploadStream({data: new crucible.ByteStream(stream(), {length: bytes.length}), contentType});
    else await client.upload({data: bytes, contentType});
  }],
  ["REST JSON", async (fetch, streaming, contentType) => {
    const client = new rest.PayloadBindingsClient({endpoint: "https://example.test", fetch});
    if (streaming) await client.uploadStream({data: new rest.ByteStream(stream(), {length: bytes.length}), contentType});
    else await client.upload({data: bytes, contentType});
  }],
];

for (const [protocol, call] of payloadCalls) {
  for (const streaming of [false, true]) {
    test(`${protocol} renamed ${streaming ? "streaming" : "buffered"} payload sends bytes and media type`, async () => {
      for (const contentType of [undefined, "image/png"]) {
        await call(async request => {
          assert.deepEqual(new Uint8Array(await request.arrayBuffer()), bytes);
          assert.equal(request.headers.get("content-type"), contentType ?? "application/octet-stream");
          return new Response(null);
        }, streaming, contentType);
      }
    });
  }
}
