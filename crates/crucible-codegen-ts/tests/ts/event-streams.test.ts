import assert from "node:assert/strict";
import { createHash, createHmac } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { test } from "node:test";
import { ChecksumError, EventStream, EventStreamError, EventsClient, EventsException, NetworkError, ThrottledError, TimeoutError, UnavailableError } from "./generated-events/src/index.js";
import type { ClientEvents, ServerEvents } from "./generated-events/src/index.js";
import { EventMessageDecoder, EventSigner, crc32, decodeEvent, decodeEventMessage, encodeEventMessage, encodeHeaders, type EventHeaderValue, type EventMessage } from "./generated-events/src/events.js";
import { EventStream as RpcEventStream, EventsClient as RpcClient } from "./generated-events-rpc/src/index.js";
import { EventStream as SignedEventStream, EventsClient as SignedClient } from "./generated-events-signed/src/index.js";

interface VectorHeader { name: string; type: string; value: unknown }
const vectors: {
  positive: { name: string; hex: string; headers: VectorHeader[]; payloadUtf8: string }[];
  negative: { name: string; hex: string }[];
  signing: { epochSeconds: number; region: string; service: string; priorSignature: string; stringToSign: string; secretAccessKey: string; signature: string };
} = JSON.parse(readFileSync(new URL("../../../../../tests/conformance/event-streams.json", import.meta.url), "utf8"));

const hexBytes = (text: string) => Uint8Array.from(text.match(/../g) ?? [], pair => parseInt(pair, 16));
const hex = (bytes: Uint8Array) => Array.from(bytes, b => b.toString(16).padStart(2, "0")).join("");
const utf8 = (text: string) => new TextEncoder().encode(text);
const fromBase64 = (text: string) => Uint8Array.from(atob(text), c => c.charCodeAt(0));

function vectorHeader(header: VectorHeader): { name: string; value: EventHeaderValue } {
  const value = header.value;
  switch (header.type) {
    case "boolean": return { name: header.name, value: { type: "boolean", value: value as boolean } };
    case "byte": case "short": case "integer": return { name: header.name, value: { type: header.type, value: value as number } };
    case "long": return { name: header.name, value: { type: "long", value: BigInt(value as string) } };
    case "blob": return { name: header.name, value: { type: "blob", value: fromBase64(value as string) } };
    case "uuid": return { name: header.name, value: { type: "uuid", value: hexBytes(value as string) } };
    case "timestamp": return { name: header.name, value: { type: "timestamp", value: value as number } };
    default: return { name: header.name, value: { type: "string", value: value as string } };
  }
}

test("crc32 matches the check value", () => {
  assert.equal(crc32(utf8("123456789")), 0xcbf43926);
});

test("published messages decode and re-encode byte for byte", () => {
  for (const vector of vectors.positive) {
    const bytes = hexBytes(vector.hex);
    const message = decodeEventMessage(bytes);
    const expected: EventMessage = { headers: vector.headers.map(vectorHeader), payload: utf8(vector.payloadUtf8) };
    assert.deepEqual(message, expected, vector.name);
    assert.equal(hex(encodeEventMessage(expected)), vector.hex, vector.name);
  }
});

test("published invalid messages are rejected", () => {
  for (const vector of vectors.negative) assert.throws(() => decodeEventMessage(hexBytes(vector.hex)), /invalid event stream message/, vector.name);
});

test("refusals of a received message do not repeat the sender's text", () => {
  const sent = "<script>".repeat(1000);
  const unknown = { headers: [{ name: ":message-type", value: { type: "string", value: sent } as EventHeaderValue }], payload: new Uint8Array() };
  assert.throws(() => decodeEvent({ member: "events", events: {} }, unknown), /^TypeError: unknown :message-type$/);
  // A header whose value type byte (here 200) no message defines.
  const header = Uint8Array.of(1, 0x78, 200);
  const frame = new Uint8Array(16 + header.length);
  const view = new DataView(frame.buffer);
  view.setUint32(0, frame.length); view.setUint32(4, header.length); view.setUint32(8, crc32(frame.subarray(0, 8)));
  frame.set(header, 12);
  view.setUint32(frame.length - 4, crc32(frame.subarray(0, frame.length - 4)));
  assert.throws(() => decodeEventMessage(frame), /^TypeError: invalid event stream message: unknown header value type$/);
});

test("messages reassemble from any chunking", () => {
  const all = vectors.positive.map(vector => hexBytes(vector.hex));
  const joined = new Uint8Array(all.reduce((size, bytes) => size + bytes.length, 0));
  let offset = 0;
  for (const bytes of all) { joined.set(bytes, offset); offset += bytes.length; }
  for (const size of [1, 3, 7, 64, joined.length]) {
    const decoder = new EventMessageDecoder();
    const decoded: string[] = [];
    for (let at = 0; at < joined.length; at += size) {
      decoder.push(joined.subarray(at, at + size));
      for (let message = decoder.next(); message; message = decoder.next()) decoded.push(hex(encodeEventMessage(message)));
    }
    assert.ok(decoder.empty);
    assert.deepEqual(decoded, vectors.positive.map(vector => vector.hex), `chunks of ${size}`);
  }
});

test("the published event signing string to sign, and the chained signature", async () => {
  const vector = vectors.signing;
  const message = encodeEventMessage({ headers: [{ name: "some-header", value: { type: "string", value: "value" } }], payload: utf8("test payload") });
  const signer = new EventSigner({ accessKeyId: "AKID", secretAccessKey: vector.secretAccessKey }, { region: vector.region, name: vector.service }, vector.priorSignature);
  assert.equal(await signer.stringToSign(message, vector.epochSeconds), vector.stringToSign);
  const signed = await signer.sign(message, vector.epochSeconds * 1000 + 999);
  assert.equal(signer.prior, vector.signature);
  assert.deepEqual(signed.headers, [
    { name: ":chunk-signature", value: { type: "blob", value: hexBytes(vector.signature) } },
    { name: ":date", value: { type: "timestamp", value: vector.epochSeconds * 1000 } },
  ]);
});

type Handler = (request: IncomingMessage, response: ServerResponse) => Promise<void>;
async function serve(handler: Handler): Promise<{ endpoint: string; close(): Promise<void> }> {
  const failures: unknown[] = [];
  const pending: Promise<void>[] = [];
  const server = createServer((request, response) => {
    pending.push(handler(request, response).catch(error => { failures.push(error); response.destroy(); }));
  });
  await new Promise<void>(resolve => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert.ok(address !== null && typeof address === "object");
  return {
    endpoint: `http://127.0.0.1:${address.port}`,
    async close() {
      await Promise.all(pending);
      server.closeAllConnections();
      await new Promise<void>(resolve => server.close(() => resolve()));
      if (failures.length > 0) throw failures[0];
    },
  };
}

async function* messages(request: IncomingMessage): AsyncGenerator<EventMessage> {
  const decoder = new EventMessageDecoder();
  for await (const chunk of request) {
    decoder.push(new Uint8Array(chunk));
    for (let message = decoder.next(); message; message = decoder.next()) yield message;
  }
}

function header(message: EventMessage, name: string): EventHeaderValue | undefined {
  return message.headers.find(item => item.name === name)?.value;
}
const str = (value: string): EventHeaderValue => ({ type: "string", value });
function event(type: string, payload: Uint8Array | string, headers: { name: string; value: EventHeaderValue }[] = [], contentType = "application/json"): Uint8Array {
  return encodeEventMessage({
    headers: [{ name: ":message-type", value: str("event") }, { name: ":event-type", value: str(type) }, { name: ":content-type", value: str(contentType) }, ...headers],
    payload: typeof payload === "string" ? utf8(payload) : payload,
  });
}
function start(response: ServerResponse, headers: Record<string, string> = {}): void {
  response.writeHead(200, { "content-type": "application/vnd.amazon.eventstream", ...headers });
  response.flushHeaders();
}
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));

test("event response checksums cover framed bytes and reject a mismatching HTTP checksum", async () => {
  for (const mismatch of [false, true]) {
    const bytes = event("structured", '{"text":"checksum","itemCount":1}');
    const value = crc32(bytes);
    const encoded = btoa(String.fromCharCode(value >>> 24, (value >>> 16) & 255, (value >>> 8) & 255, value & 255));
    const verified: string[] = [];
    const server = await serve(async (request, response) => {
      assert.equal(request.headers["x-amz-checksum-mode"], "ENABLED");
      start(response, { "x-amz-checksum-crc32": mismatch ? "AAAAAA==" : encoded });
      await write(response, bytes.subarray(0, 7));
      await write(response, bytes.subarray(7));
      response.end();
    });
    try {
      const client = new EventsClient({ endpoint: server.endpoint, checksumValidated: (operation, algorithm) => verified.push(`${operation}:${algorithm}`) });
      const output = await client.subscribeChecksums({ channel: "news", checksumMode: "ENABLED" });
      const consume = async () => { for await (const item of output.events) assert.deepEqual(item, { structured: { text: "checksum", count: 1 } }); };
      if (mismatch) {
        await assert.rejects(consume(), (error: unknown) => error instanceof ChecksumError);
        assert.deepEqual(verified, []);
      } else {
        await consume();
        assert.deepEqual(verified, ["SubscribeChecksums:CRC32"]);
      }
    } finally { await server.close(); }
  }
});
async function write(response: ServerResponse, bytes: Uint8Array): Promise<boolean> {
  if (response.destroyed) return false;
  if (!response.write(bytes)) {
    await new Promise<void>(resolve => {
      const done = () => { response.off("drain", done); response.off("close", done); resolve(); };
      response.once("drain", done); response.once("close", done);
    });
  }
  return !response.destroyed;
}

async function* clientEvents(): AsyncGenerator<ClientEvents> {
  yield { message: { id: "m1", sequence: 9007199254740993n, urgent: true, sentAt: new Date(1704164645006), text: "héllo" } };
  yield { data: { byteValue: -1, shortValue: -300, intValue: 70000, tag: new Uint8Array([1, 2]), kind: "beta", bytes: new Uint8Array([0, 255]) } };
  yield { structured: { text: "plain", count: 3, nested: { name: "n", values: [1, 2] } } };
  yield { ping: {} };
}

/** What clientEvents must look like on the wire. `count` is the JSON key of a @jsonName member. */
function checkClientEvents(received: EventMessage[], count: string): void {
  const kind = (message: EventMessage, name: string) => (header(message, name) as { value: string } | undefined)?.value;
  assert.deepEqual(received.map(message => [kind(message, ":message-type"), kind(message, ":event-type")]),
    [["event", "message"], ["event", "data"], ["event", "structured"], ["event", "ping"]]);
  const [message, data, structured, ping] = received;
  assert.equal(kind(message, ":content-type"), "text/plain");
  assert.deepEqual(header(message, "id"), str("m1"));
  assert.deepEqual(header(message, "sequence"), { type: "long", value: 9007199254740993n });
  assert.deepEqual(header(message, "urgent"), { type: "boolean", value: true });
  assert.deepEqual(header(message, "sentAt"), { type: "timestamp", value: 1704164645006 });
  assert.equal(new TextDecoder().decode(message.payload), "héllo");
  assert.equal(kind(data, ":content-type"), "application/octet-stream");
  assert.deepEqual(header(data, "byteValue"), { type: "byte", value: -1 });
  assert.deepEqual(header(data, "shortValue"), { type: "short", value: -300 });
  assert.deepEqual(header(data, "intValue"), { type: "integer", value: 70000 });
  assert.deepEqual(header(data, "tag"), { type: "blob", value: new Uint8Array([1, 2]) });
  assert.deepEqual(header(data, "kind"), str("beta"));
  assert.deepEqual(data.payload, new Uint8Array([0, 255]));
  assert.equal(kind(structured, ":content-type"), "application/json");
  assert.deepEqual(JSON.parse(new TextDecoder().decode(structured.payload)), { text: "plain", [count]: 3, nested: { name: "n", values: [1, 2] } });
  assert.equal(new TextDecoder().decode(ping.payload), "{}");
}

test("input events are framed with their header and payload bindings over HTTP", async () => {
  let received: EventMessage[] = [];
  const server = await serve(async (request, response) => {
    assert.equal(request.method, "POST");
    assert.equal(request.url, "/channels/general/publish");
    assert.equal(request.headers["content-type"], "application/vnd.amazon.eventstream");
    assert.equal(request.headers["transfer-encoding"], "chunked");
    for await (const message of messages(request)) received.push(message);
    response.writeHead(200, { "content-type": "application/json" }); response.end('{"accepted":4}');
  });
  try {
    const output = await new EventsClient({ endpoint: server.endpoint }).publish({ channel: "general", events: new EventStream(clientEvents()) });
    assert.equal(output.accepted, 4n);
    checkClientEvents(received, "itemCount");
    // A plain (async) iterable is accepted too.
    received = [];
    await new EventsClient({ endpoint: server.endpoint }).publish({ channel: "general", events: [{ ping: {} }] as unknown as EventStream<ClientEvents> });
    assert.equal(received.length, 1);
  } finally { await server.close(); }
});

test("output events arrive in order and a modeled exception is thrown as its error class", async () => {
  const server = await serve(async (request, response) => {
    assert.equal(request.url, "/channels/news/subscribe?limit=5");
    for await (const _ of request) { /* drain */ }
    start(response, { "x-channel": "news", "x-amzn-requestid": "req-1" });
    const first = event("message", "first", [{ name: "id", value: str("a") }, { name: "sequence", value: { type: "long", value: 1n } }, { name: "sentAt", value: { type: "timestamp", value: 1000 } }], "text/plain");
    // One message split across chunks, and two messages in one chunk.
    await write(response, first.subarray(0, 5)); await sleep(10); await write(response, first.subarray(5));
    const two = event("structured", '{"text":"two","itemCount":2,"at":1.5}');
    const three = event("payload", '{"name":"p","values":[7]}', [{ name: "kind", value: str("alpha") }]);
    const joined = new Uint8Array(two.length + three.length); joined.set(two); joined.set(three, two.length);
    await write(response, joined);
    await write(response, event("future", '{"x":1}'));
    await write(response, encodeEventMessage({
      headers: [{ name: ":message-type", value: str("exception") }, { name: ":exception-type", value: str("throttled") }, { name: ":content-type", value: str("application/json") }],
      payload: utf8('{"message":"slow down","retryAfterSeconds":3}'),
    }));
    response.end();
  });
  try {
    const output = await new EventsClient({ endpoint: server.endpoint }).subscribe({ channel: "news", limit: 5 });
    assert.equal(output.channel, "news");
    const seen: ServerEvents[] = [];
    let thrown: unknown;
    try { for await (const item of output.events) seen.push(item); } catch (error) { thrown = error; }
    assert.equal(seen.length, 4);
    assert.deepEqual(seen[0], { message: { id: "a", sequence: 1n, sentAt: new Date(1000), text: "first" } });
    assert.deepEqual(seen[1], { structured: { text: "two", count: 2, at: new Date(1500) } });
    assert.deepEqual(seen[2], { payload: { kind: "alpha", item: { name: "p", values: [7] } } });
    assert.deepEqual(seen[3], { $unknown: { name: "future", json: '{"x":1}' } });
    assert.ok(thrown instanceof ThrottledError, String(thrown));
    assert.ok(thrown instanceof EventsException);
    assert.equal(thrown.message, "slow down");
    assert.equal(thrown.retryAfterSeconds, 3);
    assert.equal(thrown.$metadata.requestId, "req-1");
    assert.deepEqual(thrown.$retryable, { throttling: true });
  } finally { await server.close(); }
});

test("unmodeled errors and corrupt frames end the stream", async () => {
  for (const corrupt of [false, true]) {
    const server = await serve(async (request, response) => {
      for await (const _ of request) { /* drain */ }
      start(response);
      await write(response, event("ping", "{}"));
      const last = encodeEventMessage({ headers: [{ name: ":message-type", value: str("error") }, { name: ":error-code", value: str("InternalFailure") }, { name: ":error-message", value: str("boom") }], payload: new Uint8Array() });
      if (corrupt) last[last.length - 1] ^= 1;
      response.end(last);
    });
    try {
      const output = await new EventsClient({ endpoint: server.endpoint }).subscribe({ channel: "c" });
      const iterator = output.events[Symbol.asyncIterator]();
      assert.deepEqual((await iterator.next()).value, { ping: {} });
      if (corrupt) await assert.rejects(iterator.next(), /checksum mismatch/);
      else await assert.rejects(iterator.next(), (error: unknown) => error instanceof EventStreamError && error.code === "InternalFailure" && error.message === "boom");
      assert.equal((await iterator.next()).done, true);
    } finally { await server.close(); }
  }
});

test("an HTTP error before the stream is the operation's modeled error", async () => {
  const server = await serve(async (request, response) => {
    for await (const _ of request) { /* drain */ }
    response.writeHead(503, { "x-amzn-errortype": "UnavailableError", "content-type": "application/json" });
    response.end('{"message":"later"}');
  });
  try {
    await assert.rejects(new EventsClient({ endpoint: server.endpoint, retry: false }).subscribe({ channel: "c" }), (error: unknown) => error instanceof UnavailableError && error.message === "later");
  } finally { await server.close(); }
});

test("a slow reader holds the sender back", async () => {
  const total = 64;
  let sent = 0;
  const server = await serve(async (request, response) => {
    for await (const _ of request) { /* drain */ }
    start(response);
    const large = event("message", new Uint8Array(1 << 20).fill(97), [{ name: "id", value: str("x") }], "text/plain");
    for (let index = 0; index < total; index++) {
      if (!await write(response, large)) return;
      sent++;
    }
    response.end();
  });
  try {
    const output = await new EventsClient({ endpoint: server.endpoint }).subscribe({ channel: "c" });
    const iterator = output.events[Symbol.asyncIterator]();
    assert.equal((await iterator.next()).done, false);
    await sleep(300);
    assert.ok(sent < total / 2, `the server sent ${sent} of ${total} 1 MiB events unread`);
    let received = 1;
    while (!(await iterator.next()).done) received++;
    assert.equal(received, total);
  } finally { await server.close(); }
});

test("a slow server holds the event source back", async () => {
  const total = 64;
  let pulled = 0;
  let release!: () => void;
  const released = new Promise<void>(resolve => { release = resolve; });
  const server = await serve(async (request, response) => {
    request.pause();
    await released;
    request.resume();
    let count = 0;
    for await (const _ of messages(request)) count++;
    response.writeHead(200, { "content-type": "application/json" }); response.end(`{"accepted":${count}}`);
  });
  async function* source(): AsyncGenerator<ClientEvents> {
    for (let index = 0; index < total; index++) { pulled++; yield { message: { id: "x", text: "a".repeat(1 << 20) } }; }
  }
  try {
    const call = new EventsClient({ endpoint: server.endpoint }).publish({ channel: "c", events: new EventStream(source()) });
    await sleep(300);
    assert.ok(pulled < total / 2, `${pulled} of ${total} 1 MiB events were pulled unsent`);
    release();
    assert.equal((await call).accepted, BigInt(total));
  } finally { await server.close(); }
});

test("cancellation, deadlines and leaving the loop release the connection", async () => {
  for (const mode of ["abort", "timeout", "break"] as const) {
    let closed!: () => void;
    const disconnected = new Promise<void>(resolve => { closed = resolve; });
    const server = await serve(async (request, response) => {
      for await (const _ of request) { /* drain */ }
      response.on("close", closed);
      start(response);
      await write(response, event("ping", "{}"));
      await disconnected;
    });
    try {
      const abort = new AbortController();
      const client = new EventsClient({ endpoint: server.endpoint, timeoutMs: mode === "timeout" ? 300 : undefined });
      const output = await client.subscribe({ channel: "c" }, { signal: abort.signal });
      if (mode === "break") {
        for await (const item of output.events) { assert.deepEqual(item, { ping: {} }); break; }
      } else {
        const iterator = output.events[Symbol.asyncIterator]();
        assert.deepEqual((await iterator.next()).value, { ping: {} });
        if (mode === "abort") setTimeout(() => abort.abort(new Error("stop listening")), 50);
        await assert.rejects(iterator.next(), mode === "abort" ? /stop listening/ : (error: unknown) => error instanceof TimeoutError);
      }
      await disconnected;
    } finally { await server.close(); }
  }
});

test("an unread stream can be cancelled, releasing the connection", async () => {
  let closed!: () => void;
  const disconnected = new Promise<void>(resolve => { closed = resolve; });
  const server = await serve(async (request, response) => {
    for await (const _ of request) { /* drain */ }
    response.on("close", closed);
    start(response);
    await write(response, event("ping", "{}"));
    await disconnected;
  });
  try {
    const output = await new EventsClient({ endpoint: server.endpoint }).subscribe({ channel: "c" });
    await output.events.cancel();
    assert.throws(() => output.events[Symbol.asyncIterator](), /read once/);
    await disconnected;
  } finally { await server.close(); }
});

test("a failing event source aborts the request", async () => {
  let complete = false;
  let count = 0;
  const server = await serve(async request => {
    try { for await (const _ of messages(request)) count++; complete = true; } catch { /* aborted */ }
  });
  async function* source(): AsyncGenerator<ClientEvents> {
    yield { ping: {} };
    await sleep(100);
    throw new Error("source failed");
  }
  try {
    await assert.rejects(new EventsClient({ endpoint: server.endpoint }).publish({ channel: "c", events: new EventStream(source()) }), (error: unknown) => error instanceof NetworkError);
  } finally { await server.close(); }
  assert.equal(count, 1);
  assert.equal(complete, false);
});

test("RPC protocols carry the other members in initial messages", async () => {
  const requests: string[] = [];
  let received: EventMessage[] = [];
  const server = await serve(async (request, response) => {
    const target = String(request.headers["x-amz-target"]);
    requests.push(target);
    assert.equal(request.url, "/");
    if (target === "Events.Publish") {
      assert.equal(request.headers["content-type"], "application/vnd.amazon.eventstream");
      for await (const message of messages(request)) received.push(message);
      response.writeHead(200, { "content-type": "application/x-amz-json-1.0" }); response.end('{"accepted":4}');
      return;
    }
    let body = "";
    for await (const chunk of request) body += new TextDecoder().decode(chunk);
    start(response);
    if (JSON.parse(body).limit === 2) await write(response, event("initial-response", '{"channel":"news"}'));
    await write(response, event("ping", "{}"));
    response.end();
  });
  try {
    const client = new RpcClient({ endpoint: server.endpoint });
    assert.equal((await client.publish({ channel: "general", events: new RpcEventStream(clientEvents()) })).accepted, 4n);
    const [initial, ...events] = received;
    assert.deepEqual(header(initial, ":event-type"), str("initial-request"));
    assert.deepEqual(JSON.parse(new TextDecoder().decode(initial.payload)), { channel: "general" });
    checkClientEvents(events, "count");
    for (const limit of [2, undefined]) {
      const output = await client.subscribe({ channel: "news", limit });
      assert.equal(output.channel, limit === 2 ? "news" : undefined);
      const seen = [];
      for await (const item of output.events) seen.push(item);
      assert.deepEqual(seen, [{ ping: {} }]);
    }
    assert.deepEqual(requests, ["Events.Publish", "Events.Subscribe", "Events.Subscribe"]);
  } finally { await server.close(); }
});

const hmac = (key: Uint8Array | string, value: Uint8Array | string) => createHmac("sha256", key).update(value).digest();
const sha256 = (bytes: Uint8Array) => hex(new Uint8Array(createHash("sha256").update(bytes).digest()));

test("SigV4 signs every message in a chain from the request signature", async () => {
  const inner: EventMessage[] = [];
  let lastEmpty = false;
  const server = await serve(async (request, response) => {
    assert.equal(request.headers["x-amz-content-sha256"], "STREAMING-AWS4-HMAC-SHA256-EVENTS");
    const authorization = String(request.headers.authorization);
    assert.match(authorization, /\/us-west-2\/events\/aws4_request/);
    let prior = /Signature=([0-9a-f]+)/.exec(authorization)![1];
    for await (const envelope of messages(request)) {
      const date = header(envelope, ":date"), signature = header(envelope, ":chunk-signature");
      assert.ok(date?.type === "timestamp" && signature?.type === "blob");
      const stamp = new Date(date.value).toISOString().replace(/[:-]|\.\d{3}/g, "");
      const toSign = `AWS4-HMAC-SHA256-PAYLOAD\n${stamp}\n${stamp.slice(0, 8)}/us-west-2/events/aws4_request\n${prior}\n${sha256(encodeHeaders([{ name: ":date", value: date }]))}\n${sha256(envelope.payload)}`;
      let key = hmac(utf8("AWS4secret"), stamp.slice(0, 8)); key = hmac(key, "us-west-2"); key = hmac(key, "events"); key = hmac(key, "aws4_request");
      const expected = new Uint8Array(hmac(key, toSign));
      assert.equal(hex(signature.value), hex(expected));
      prior = hex(expected);
      lastEmpty = envelope.payload.length === 0;
      if (!lastEmpty) inner.push(decodeEventMessage(envelope.payload));
    }
    response.writeHead(200, { "content-type": "application/json" }); response.end('{"accepted":4}');
  });
  try {
    const client = new SignedClient({ endpoint: server.endpoint, region: "us-west-2", awsCredentials: { accessKeyId: "AKID", secretAccessKey: "secret" } });
    assert.equal((await client.publish({ channel: "general", events: new SignedEventStream(clientEvents()) })).accepted, 4n);
    assert.ok(lastEmpty, "a signed stream ends with an empty signed message");
    checkClientEvents(inner, "itemCount");
  } finally { await server.close(); }
});

test("bidirectional streams over HTTP/1.1 fetch are half duplex: the response follows the request body", async () => {
  const server = await serve(async (request, response) => {
    assert.equal(request.headers["x-room"], "lobby");
    let count = 0;
    for await (const _ of messages(request)) count++;
    start(response);
    for (let index = 1; index <= count; index++) await write(response, event("structured", `{"text":"echo ${index}"}`));
    response.end();
  });
  try {
    const output = await new EventsClient({ endpoint: server.endpoint }).chat({ room: "lobby", events: new EventStream([{ ping: {} }, { ping: {} }, { ping: {} }]) });
    const seen = [];
    for await (const item of output.events!) seen.push(item);
    assert.deepEqual(seen, [1, 2, 3].map(index => ({ structured: { text: `echo ${index}` } })));
  } finally { await server.close(); }
});

test("Node's fetch exchanges events interactively on one HTTP/1.1 connection", async () => {
  // The server answers the first event before the request body is complete.
  const server = await serve(async (request, response) => {
    const iterator = messages(request)[Symbol.asyncIterator]();
    await iterator.next();
    start(response);
    await write(response, event("ping", "{}"));
    let count = 0;
    while (!(await iterator.next()).done) { count++; await write(response, event("structured", `{"text":"echo ${count}"}`)); }
    response.end();
  });
  const queue: ((value: ClientEvents | undefined) => void)[] = [];
  const waiting: (ClientEvents | undefined)[] = [];
  const push = (value: ClientEvents | undefined) => { const resolve = queue.shift(); if (resolve) resolve(value); else waiting.push(value); };
  async function* source(): AsyncGenerator<ClientEvents> {
    for (;;) {
      const next = waiting.length > 0 ? waiting.shift() : await new Promise<ClientEvents | undefined>(resolve => queue.push(resolve));
      if (next === undefined) return;
      yield next;
    }
  }
  try {
    push({ ping: {} });
    const output = await new EventsClient({ endpoint: server.endpoint }).chat({ room: "lobby", events: new EventStream(source()) });
    const iterator = output.events![Symbol.asyncIterator]();
    assert.deepEqual((await iterator.next()).value, { ping: {} });
    for (let round = 1; round <= 3; round++) {
      push({ ping: {} });
      assert.deepEqual((await iterator.next()).value, { structured: { text: `echo ${round}` } });
    }
    push(undefined);
    assert.equal((await iterator.next()).done, true);
  } finally { await server.close(); }
});

test("unknown and malformed exception frames share the service base", async () => {
  for (const [name, body, expected] of [
    ["FutureException", '{"message":"future"}', "FutureException"],
    ["__proto__", '{"message":"future"}', "__proto__"],
    ["constructor", '{"message":"future"}', "constructor"],
    ["throttled", '{bad', "ThrottledError"],
    ["throttled", '{"message":42}', "ThrottledError"],
  ]) {
    const server = await serve(async (request, response) => {
      for await (const _ of request) { /* drain */ }
      start(response);
      response.end(encodeEventMessage({ headers: [
        { name: ":message-type", value: str("exception") },
        { name: ":exception-type", value: str(name) },
        { name: ":content-type", value: str("application/json") },
      ], payload: utf8(body) }));
    });
    try {
      const output = await new EventsClient({ endpoint: server.endpoint }).subscribe({ channel: "c" });
      await assert.rejects(output.events[Symbol.asyncIterator]().next(), (error: unknown) => {
        if (!(error instanceof EventsException) || error.name !== expected) return false;
        if (name === "throttled") assert.deepEqual(error.$retryable, { throttling: true });
        else assert.equal(error.message, "future");
        return true;
      });
    } finally { await server.close(); }
  }
});
