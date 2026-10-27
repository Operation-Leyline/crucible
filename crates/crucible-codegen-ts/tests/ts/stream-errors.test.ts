import assert from "node:assert/strict";
import { test } from "node:test";
import { createServer } from "node:http";
import { StreamErrorsClient, BlobProblem, DocumentProblem, NestedProblem, StreamProtocolError, TextProblem } from "./generated-stream-errors/src/index.js";

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

// #431's acceptance: the generated client over a real local listener with the
// default global fetch and retry policy, so no Fetch or reader mock stands in
// for the wire. Resolves to the number of requests the listener answered.
async function served(status: number, headers: Record<string, string>, body: string | Uint8Array,
  run: (client: StreamErrorsClient) => Promise<void>): Promise<number> {
  let requests = 0;
  const server = createServer((_request, response) => { requests++; response.writeHead(status, headers); response.end(body); });
  await new Promise<void>(resolve => { server.listen(0, "127.0.0.1", resolve); });
  const address = server.address();
  assert(address !== null && typeof address !== "string");
  try {
    await run(new StreamErrorsClient({ endpoint: `http://127.0.0.1:${address.port}` }));
    return requests;
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) => { server.close(error => error ? reject(error) : resolve()); });
  }
}

/** A stream that yields one item and then ends with a late `crucible.error`. */
async function nativeLate(type: string, status: number, error: string, check: (error: unknown) => void, ending = "\n") {
  const text = `data: {"ready":"first"}\n\nevent: crucible.error\ndata: {"type":"${type}","status":${status},"error":${error}}\n\n`;
  const requests = await served(200, headers, text.replaceAll("\n", ending), async client => {
    const stream = await client.watch();
    assert.deepEqual((await stream.next()).value, { ready: "first" });
    await assert.rejects(stream.next(), error => { check(error); return true; });
  });
  assert.equal(requests, 1, "a late error must not replay the opened request");
}

test("native HTTP: late blob payloads decode to exact Uint8Array bytes", async () => {
  for (const ending of ["\n", "\r\n", "\r"]) {
    for (const [data, bytes] of [["AAH/", [0, 1, 255]], ["", []]] as const) {
      await nativeLate("BlobProblem", 421, `{"reason":"needed","data":"${data}"}`, error => {
        assert(error instanceof BlobProblem);
        assert(error.data instanceof Uint8Array, `late blob is ${typeof error.data}`);
        assert.deepEqual([...error.data], bytes);
        assert.equal(error.docUrl, "https://docs.example.test/errors/BlobProblem/reason-needed");
      }, ending);
    }
  }
});

// No request count here: the Fetch standard itself retries a 421 (Misdirected
// Request) once on a new connection, below the generated client.
test("native HTTP: opening blob errors keep raw body bytes and header members", async () => {
  await served(421, {
    "content-type": "application/octet-stream", "x-crucible-error": "BlobProblem", "x-reason": "needed", "x-time": "2026-10-26T12:00:00Z",
  }, new Uint8Array([0, 1, 255]), async client => {
    await assert.rejects(client.watch(), error => {
      assert(error instanceof BlobProblem);
      assert(error.data instanceof Uint8Array);
      assert.deepEqual([...error.data], [0, 1, 255]);
      assert.equal(error.reason, "needed");
      assert.equal(error.when?.toISOString(), "2026-10-26T12:00:00.000Z");
      assert.equal(error.docUrl, "https://docs.example.test/errors/BlobProblem/reason-needed");
      return true;
    });
  });
});

test("native HTTP: late header-bound members decode from the JSON document", async () => {
  await nativeLate("BlobProblem", 421, `{"reason":"needed","when":"2026-10-26T12:00:00Z"}`, error => {
    assert(error instanceof BlobProblem);
    assert.equal(error.reason, "needed");
    assert(error.when instanceof Date);
    assert.equal(error.when.toISOString(), "2026-10-26T12:00:00.000Z");
    assert.equal(error.data, undefined);
  });
  await nativeLate("TextProblem", 409, `{"note":"header text"}`, error => {
    assert(error instanceof TextProblem);
    assert.equal(error.note, "header text");
    assert.equal(error.details, undefined);
  });
});

test("native HTTP: late string payloads stay strings", async () => {
  for (const details of ["plain \u00e9 text", ""]) {
    await nativeLate("TextProblem", 409, JSON.stringify({ note: "n", details }), error => {
      assert(error instanceof TextProblem);
      assert.equal(error.details, details);
      assert.equal(error.note, "n");
    });
  }
});

test("native HTTP: late document payloads keep exact values, null and omission", async () => {
  await nativeLate("DocumentProblem", 423, `{"data":{"big":12345678901234567890,"list":[1,"AAH/",null,true]}}`, error => {
    assert(error instanceof DocumentProblem);
    assert.deepEqual(error.data, { big: 12345678901234567890n, list: [1, "AAH/", null, true] });
  });
  await nativeLate("DocumentProblem", 423, `{"data":null}`, error => {
    assert(error instanceof DocumentProblem);
    assert.equal(error.data, null);
  });
  await nativeLate("DocumentProblem", 423, `{}`, error => {
    assert(error instanceof DocumentProblem);
    assert.equal(error.data, undefined);
  });
});
