import assert from "node:assert/strict";
import { test } from "node:test";
import { StreamErrorsClient, BlobProblem, NestedProblem, StreamProtocolError } from "./generated-stream-errors/src/index.js";

const headers = { "content-type": "text/event-stream; crucible-stream=1" };
function late(type: string, error: object) {
  const text = `event: crucible.error\ndata: ${JSON.stringify({ type, status: type === "BlobProblem" ? 421 : 422, error })}\n\n`;
  return new StreamErrorsClient({ endpoint: "https://example.test", fetch: async () => new Response(text, { headers }) });
}

test("late errors use document blob conversion and header timestamp formats", async () => {
  for (const data of ["AAH/", ""]) {
    const stream = await late("BlobProblem", { data, when: "2026-10-26T12:00:00Z" }).watch();
    await assert.rejects(stream.next(), error => {
      assert(error instanceof BlobProblem);
      assert(error.data instanceof Uint8Array);
      assert.deepEqual(error.data, data ? new Uint8Array([0, 1, 255]) : new Uint8Array());
      assert(error.when instanceof Date);
      assert.equal(error.when.toISOString(), "2026-10-26T12:00:00.000Z");
      assert.equal(error.docUrl, "https://docs.example.test/errors/BlobProblem/reason-needed");
      return true;
    });
  }
});

// Nested blobs already decode as base64 through their shared schema: .cru cannot
// make a nested shape an HTTP blob payload, so a distinct nested document schema
// arises only from IR front ends and is covered by the Rust golden test
// late_document_schemas_follow_reused_http_payloads_through_recursive_containers.
test("late nested blobs decode through lists, maps and recursive structures", async () => {
  const envelope = { data: "AAH/" };
  const fields = { envelope: { ...envelope, next: envelope }, entries: [envelope], mapping: { key: envelope } };
  const stream = await late("NestedProblem", fields).watch();
  await assert.rejects(stream.next(), error => {
    assert(error instanceof NestedProblem);
    for (const decoded of [error.envelope, error.entries?.[0], error.mapping?.key, error.envelope?.next]) {
      assert(decoded?.data instanceof Uint8Array);
      assert.deepEqual(decoded.data, new Uint8Array([0, 1, 255]));
    }
    return true;
  });
});

test("late blob omission and nullable values retain presence, malformed base64 fails", async () => {
  for (const fields of [{}, { data: null }]) {
    const stream = await late("BlobProblem", fields).watch();
    await assert.rejects(stream.next(), error => {
      assert(error instanceof BlobProblem);
      assert.equal(error.data, "data" in fields ? null : undefined);
      return true;
    });
  }
  for (const data of ["!", "A", [0, 1, 255], 17]) {
    const stream = await late("BlobProblem", { data }).watch();
    await assert.rejects(stream.next(), error => error instanceof StreamProtocolError);
  }
});

test("ordinary HTTP blob payloads retain raw bytes and HTTP header decoding", async () => {
  const client = new StreamErrorsClient({ endpoint: "https://example.test", fetch: async () => new Response(new Uint8Array([0, 1, 255]), {
    status: 421, headers: { "x-crucible-error": "BlobProblem", "x-reason": "needed", "x-time": "2026-10-26T12:00:00Z" },
  }) });
  await assert.rejects(client.watch(), error => {
    assert(error instanceof BlobProblem);
    assert.deepEqual(error.data, new Uint8Array([0, 1, 255]));
    assert.equal(error.when?.toISOString(), "2026-10-26T12:00:00.000Z");
    return true;
  });
});
