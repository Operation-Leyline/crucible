import assert from "node:assert/strict";
import { test } from "node:test";
import { StreamsClient, StreamProtocolError, TimeoutError, WorkFailed, NetworkError } from "./generated-streams/src/index.js";

const encoder = new TextEncoder();
const headers = { "content-type": "text/event-stream; charset=utf-8; crucible-stream=1", "x-request-id": "stream-1" };
const complete = "event: crucible.complete\ndata: {}\n\n";
const message = (text: string) => `event: message\ndata: ${JSON.stringify({ ready: text })}\n\n`;
const client = (fetch: (request: Request) => Promise<Response>) => new StreamsClient({ endpoint: "https://example.test", fetch });

function body(text: string, split = 1) {
  const bytes = encoder.encode(text);
  let at = 0;
  return new ReadableStream<Uint8Array>({ pull(controller) {
    if (at === bytes.length) controller.close();
    else { controller.enqueue(bytes.slice(at, at + split)); at = Math.min(bytes.length, at + split); }
  } });
}

test("generated stream methods parse UTF-8 and CRLF across arbitrary chunk boundaries", async () => {
  for (const split of [1, 2, 7, 4096]) {
    const api = client(async () => new Response(body((message("a🌊b") + complete).replaceAll("\n", "\r\n"), split), { headers }));
    const events = await api.watch();
    assert.equal(events.metadata.requestId, "stream-1");
    const values = [];
    for await (const event of events) values.push(event);
    assert.deepEqual(values, [{ ready: "a🌊b" }]);
  }
});

test("first item arrives while producer is still open and loop break cancels it", async () => {
  let cancelled = 0;
  const api = client(async request => {
    assert.equal(request.method, "POST");
    assert.deepEqual(JSON.parse(await request.text()), { topic: "pending" });
    return new Response(new ReadableStream({ start(controller) { controller.enqueue(encoder.encode(message("first"))); }, cancel() { cancelled++; } }), { headers });
  });
  const events = await api.publish({ topic: "pending" });
  for await (const event of events) { assert.deepEqual(event, { ready: "first" }); break; }
  assert.equal(cancelled, 1);
});

test("unconsumed cancel closes once and repeated iteration cannot repeat a call", async () => {
  let cancelled = 0, requests = 0;
  const events = await client(async () => { requests++; return new Response(new ReadableStream({ cancel() { cancelled++; } }), { headers }); }).watch();
  events[Symbol.asyncIterator]();
  assert.throws(() => events[Symbol.asyncIterator](), /one consumer/);
  await events.cancel(); await events.cancel();
  assert.equal(requests, 1); assert.equal(cancelled, 1);
});

test("cancellation prevents disclosure of events already buffered in a chunk", async () => {
  const events = await client(async () => new Response(body(message("first") + message("second") + complete, 4096), { headers })).watch();
  assert.deepEqual((await events.next()).value, { ready: "first" });
  await events.cancel(new Error("cancelled"));
  await assert.rejects(events.next(), /cancelled/);
});

test("terminal errors become generated typed exceptions during iteration", async () => {
  const text = message("first") + 'event: crucible.error\ndata: {"type":"WorkFailed","status":503,"error":{"message":"step failed"}}\n\n';
  const events = await client(async () => new Response(body(text), { headers })).watch();
  assert.deepEqual((await events.next()).value, { ready: "first" });
  await assert.rejects(events.next(), error => error instanceof WorkFailed && error.message === "step failed" && error.$metadata.status === 503);
});

test("truncation, duplicate terminals, late data and unknown controls fail explicitly", async () => {
  for (const text of [message("first"), complete + complete, complete + message("late"), "event: crucible.future\ndata: {}\n\n"]) {
    const events = await client(async () => new Response(body(text), { headers })).watch();
    await assert.rejects((async () => { for await (const _ of events) { /* consume */ } })(), error => error instanceof StreamProtocolError);
  }
});

test("zero-event success and unknown application variants are supported", async () => {
  const events = await client(async () => new Response(body(complete), { headers })).watch();
  assert.equal((await events.next()).done, true);
  const unknown = await client(async () => new Response(body('data: {"future":{"x":"y"}}\n\n' + complete), { headers })).watch();
  assert.equal((await unknown.next()).done, false);
  await unknown.cancel();
});

test("SSE comments, multiline data and CR framing do not enable replay", async () => {
  const text = ': heartbeat\rid: cursor\rretry: 1\rdata: {"ready":\rdata: "first"}\r\r' + complete;
  const events = await client(async () => new Response(body(text, 1), { headers })).watch();
  const values = [];
  for await (const value of events) values.push(value);
  assert.deepEqual(values, [{ ready: "first" }]);
});

test("timeout remains active after headers and the first item", async () => {
  let cancelled = 0;
  const api = client(async () => new Response(new ReadableStream({ start(c) { c.enqueue(encoder.encode(message("first"))); }, cancel() { cancelled++; } }), { headers }));
  const events = await api.watch({ timeoutMs: 40 });
  assert.equal((await events.next()).done, false);
  await assert.rejects(events.next(), error => error instanceof TimeoutError);
  assert.equal(cancelled, 1);
});

test("a transport failure during a retry-safe stream never repeats the request", async () => {
  let requests = 0, wire: ReadableStreamDefaultController<Uint8Array> | undefined;
  const api = client(async () => { requests++; return new Response(new ReadableStream({ start(c) { wire = c; c.enqueue(encoder.encode(message("first"))); } }), { headers }); });
  const events = await api.watch();
  await events.next(); wire!.error(new Error("connection lost"));
  await assert.rejects(events.next(), error => error instanceof NetworkError);
  assert.equal(requests, 1);
});

test("ordinary HTTP opening errors reject before returning an iterator", async () => {
  const api = client(async () => new Response('{"message":"opening failed"}', { status: 503, headers: { "content-type": "application/json", "x-crucible-error": "WorkFailed" } }));
  await assert.rejects(api.publish({ topic: "open-fail" }), error => error instanceof WorkFailed);
});

test("malformed stream values fail without copying received data into errors", async () => {
  for (const text of ['data: private-invalid-json\n\n', 'data: {"update":{"text":false}}\n\n', 'event: crucible.error\ndata: {"type":"WorkFailed","status":"private","error":{}}\n\n']) {
    const events = await client(async () => new Response(body(text), { headers })).watch();
    await assert.rejects(events.next(), error => error instanceof StreamProtocolError && !error.message.includes("private"));
  }
  const invalid = await client(async () => new Response(new Uint8Array([0xff]), { headers })).watch();
  await assert.rejects(invalid.next(), error => error instanceof StreamProtocolError && /UTF-8/.test(error.message));
});

test("event byte limits count UTF-8 and can be overridden per call", async () => {
  const api = client(async () => new Response(body(message("🌊".repeat(70)) + complete, 1), { headers }));
  const events = await api.watch({ maxStreamEventBytes: 256 });
  await assert.rejects(events.next(), error => error instanceof StreamProtocolError && /limit/.test(error.message));
  const larger = await api.watch({ maxStreamEventBytes: 512 });
  assert.deepEqual((await larger.next()).value, { ready: "🌊".repeat(70) });
  await larger.cancel();
});
