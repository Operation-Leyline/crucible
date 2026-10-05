import { test } from "node:test";
import assert from "node:assert/strict";
import { StreamingClient, ByteStream, TimeoutError, contentSha256 } from "./generated-streaming/src/index.js";

function chunks(text: string): ReadableStream<Uint8Array> {
  return new ReadableStream({ start(c) { c.enqueue(new TextEncoder().encode(text)); c.close(); } });
}

test("one-shot streaming uploads dispatch once even for an idempotent operation", async () => {
  let attempts = 0;
  const client = new StreamingClient({ endpoint: "https://stream.test", retry: { baseDelayMs: 0 }, fetch: async request => {
    attempts++; assert.equal(await request.text(), "one shot"); return new Response("{}", { status: 503 });
  } });
  await assert.rejects(client.upload({ body: new ByteStream(chunks("one shot")) }));
  assert.equal(attempts, 1);
});

test("stream replay factories run once per attempt and reproduce upload bytes", async () => {
  let factories = 0; let attempts = 0;
  const body = new ByteStream(() => { factories++; return chunks("same bytes"); });
  const client = new StreamingClient({ endpoint: "https://stream.test", retry: { baseDelayMs: 0 }, fetch: async request => {
    attempts++; assert.equal(await request.text(), "same bytes");
    return new Response(null, { status: attempts === 1 ? 503 : 204 });
  } });
  await client.upload({ body });
  assert.equal(factories, 2); assert.equal(attempts, 2);
});

test("stream deadlines remain active after the operation returns", async () => {
  let cancelled = false;
  const client = new StreamingClient({ endpoint: "https://stream.test", timeoutMs: 25, fetch: async () => new Response(new ReadableStream({ cancel() { cancelled = true; } })) });
  const output = await client.download();
  assert.ok(output.body instanceof ByteStream);
  const reader = output.body.open().getReader();
  let reason: unknown;
  try { await reader.read(); } catch (error) { reason = error; }
  assert.ok(reason instanceof TimeoutError); assert.equal(cancelled, true);
  reader.releaseLock();
});

test("caller cancellation interrupts a pending streaming read and releases the source", async () => {
  let cancelled = false;
  const abort = new AbortController();
  const source = new ReadableStream<Uint8Array>({ cancel() { cancelled = true; } });
  const client = new StreamingClient({ endpoint: "https://stream.test", fetch: async () => new Response(source) });
  const output = await client.download({ signal: abort.signal });
  const reader = output.body!.open().getReader();
  const read = reader.read(); abort.abort(new Error("stop streaming"));
  await assert.rejects(read, /stop streaming/); assert.equal(cancelled, true);
  reader.releaseLock();
});

test("unopened streaming responses can be cancelled without consuming bytes", async () => {
  let pulls = 0; let cancelled = false;
  const source = new ReadableStream<Uint8Array>({ pull() { pulls++; }, cancel() { cancelled = true; } }, { highWaterMark: 0 });
  const client = new StreamingClient({ endpoint: "https://stream.test", timeoutMs: 1000, fetch: async () => new Response(source) });
  const output = await client.download();
  assert.equal(pulls, 0); await output.body!.cancel(); assert.equal(cancelled, true); assert.equal(source.locked, false);
});

test("large streaming responses use bounded pull-through storage", async () => {
  let produced = 0; let consumed = 0; const count = 1024; const size = 65536;
  const source = new ReadableStream<Uint8Array>({ pull(controller) {
    assert.ok(produced - consumed <= 1, "producer outran consumer");
    if (produced === count) controller.close(); else { produced++; controller.enqueue(new Uint8Array(size)); }
  } }, { highWaterMark: 0 });
  const client = new StreamingClient({ endpoint: "https://stream.test", fetch: async () => new Response(source) });
  const output = await client.download(); assert.equal(produced, 0);
  const reader = output.body!.open().getReader(); let bytes = 0;
  for (;;) { const next = await reader.read(); if (next.done) break; consumed++; bytes += next.value.length; }
  assert.equal(bytes, count * size); assert.equal(source.locked, false); reader.releaseLock();
});

test("streaming digests require precomputed values without buffering", async () => {
  const digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
  let opens = 0;
  const client = new StreamingClient({ endpoint: "https://stream.test", middleware: [contentSha256], fetch: async request => {
    assert.equal(request.headers.get("x-amz-content-sha256"), digest); assert.equal(await request.text(), "abc"); return new Response(null, { status: 204 });
  } });
  await assert.rejects(client.upload({ body: new ByteStream(() => { opens++; return chunks("abc"); }) }), /precomputed/);
  assert.equal(opens, 0);
  await client.upload({ body: new ByteStream(() => { opens++; return chunks("abc"); }, { sha256: digest }) });
  assert.equal(opens, 1);
});

test("requiresLength rejects missing lengths and validates actual stream bytes", async () => {
  const client = new StreamingClient({ endpoint: "https://stream.test", retry: false, fetch: async request => {
    assert.equal(request.headers.get("content-length"), "3"); await request.arrayBuffer(); return new Response(null, { status: 204 });
  } });
  await assert.rejects(client.sized({ body: new ByteStream(chunks("abc")) }), /known length/);
  await client.sized({ body: new ByteStream(chunks("abc"), { length: 3 }) });
  await assert.rejects(client.sized({ body: new ByteStream(chunks("ab"), { length: 3 }) }), /declared length/);
});

test("invalid streaming response headers cancel the unread body", async () => {
  let cancelled = false;
  const source = new ReadableStream<Uint8Array>({ cancel() { cancelled = true; } });
  const client = new StreamingClient({ endpoint: "https://stream.test", fetch: async () => new Response(source, { headers: { "x-count": "invalid" } }) });
  await assert.rejects(client.download()); assert.equal(cancelled, true); assert.equal(source.locked, false);
});
