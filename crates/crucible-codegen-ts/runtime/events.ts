// Smithy event streams over the AWS event stream framing
// (application/vnd.amazon.eventstream): the binary message format, an
// incremental decoder, model events to and from messages, SigV4 chunk
// signatures, and the EventStream values generated clients expose.
// Edit it in the crucible repository, never in a generated package.

import { ServiceError, ServiceException, fromWire, parseJson, stringifyJson, toBase64, toWire, wireName, type ErrorSpec, type ResponseMetadata, type Schema } from "./runtime.js";
import type { AwsCredentials, SigningScope } from "./credentials.js";

/** AWS event-stream media type used for framed HTTP payloads. */
export const EVENT_STREAM_CONTENT_TYPE = "application/vnd.amazon.eventstream";
const MAX_HEADERS = 128 * 1024;
const MAX_PAYLOAD = 16 * 1024 * 1024;

/** A typed AWS event-stream header value. */
export type EventHeaderValue =
  | { readonly type: "boolean"; readonly value: boolean }
  | { readonly type: "byte" | "short" | "integer"; readonly value: number }
  | { readonly type: "long"; readonly value: bigint }
  | { readonly type: "blob" | "uuid"; readonly value: Uint8Array }
  | { readonly type: "string"; readonly value: string }
  /** Milliseconds since the Unix epoch. */
  | { readonly type: "timestamp"; readonly value: number };

/** Typed event-stream headers and a buffered payload for one frame. */
export interface EventMessage {
  readonly headers: readonly { readonly name: string; readonly value: EventHeaderValue }[];
  readonly payload: Uint8Array;
}

/** An error the model does not name: `:message-type error`, or an exception type this client does not know. The stream has ended. */
export class EventStreamError extends Error {
  constructor(readonly code: string, message: string) { super(message); this.name = "EventStreamError"; }
}

const TABLE = (() => {
  const table = new Uint32Array(256);
  for (let index = 0; index < 256; index++) {
    let value = index;
    for (let bit = 0; bit < 8; bit++) value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
    table[index] = value >>> 0;
  }
  return table;
})();

/** CRC-32 (IEEE, as zlib computes it): both of a message's checksums. */
export function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  for (const byte of bytes) crc = TABLE[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

const TYPES = ["boolean", "boolean", "byte", "short", "integer", "long", "blob", "string", "timestamp", "uuid"] as const;
const utf8 = new TextEncoder();
const text = (bytes: Uint8Array) => new TextDecoder("utf-8", { fatal: true }).decode(bytes);

function frameError(message: string): TypeError { return new TypeError(`invalid event stream message: ${message}`); }

/** Headers as they are encoded inside a message. */
export function encodeHeaders(headers: EventMessage["headers"]): Uint8Array {
  const parts: number[] = [];
  const push = (bytes: ArrayLike<number>) => { for (let at = 0; at < bytes.length; at++) parts.push(bytes[at]); };
  const integer = (value: number | bigint, size: number) => {
    const view = new DataView(new ArrayBuffer(size));
    if (size === 1) view.setInt8(0, Number(value)); else if (size === 2) view.setInt16(0, Number(value)); else if (size === 4) view.setInt32(0, Number(value)); else view.setBigInt64(0, BigInt(value));
    push(new Uint8Array(view.buffer));
  };
  for (const { name, value } of headers) {
    const encoded = utf8.encode(name);
    if (encoded.length === 0 || encoded.length > 255) throw frameError("header names are 1 to 255 bytes");
    parts.push(encoded.length); push(encoded);
    switch (value.type) {
      case "boolean": parts.push(value.value ? 0 : 1); break;
      case "byte": parts.push(2); integer(value.value, 1); break;
      case "short": parts.push(3); integer(value.value, 2); break;
      case "integer": parts.push(4); integer(value.value, 4); break;
      case "long": parts.push(5); integer(value.value, 8); break;
      case "timestamp": parts.push(8); integer(BigInt(Math.trunc(value.value)), 8); break;
      case "uuid":
        if (value.value.length !== 16) throw frameError("a UUID header is 16 bytes");
        parts.push(9); push(value.value); break;
      case "blob": case "string": {
        const bytes = value.type === "string" ? utf8.encode(value.value) : value.value;
        if (bytes.length > 0xffff) throw frameError("header values are at most 65535 bytes");
        parts.push(value.type === "blob" ? 6 : 7, bytes.length >> 8, bytes.length & 0xff); push(bytes);
        break;
      }
    }
  }
  return Uint8Array.from(parts);
}

/** Encode one event-stream frame including prelude and message CRC32 checksums. */
export function encodeEventMessage(message: EventMessage): Uint8Array {
  const headers = encodeHeaders(message.headers);
  if (headers.length > MAX_HEADERS) throw frameError("headers exceed 128 KiB");
  if (message.payload.length > MAX_PAYLOAD) throw frameError("payload exceeds 16 MiB");
  const total = 16 + headers.length + message.payload.length;
  const bytes = new Uint8Array(total);
  const view = new DataView(bytes.buffer);
  view.setUint32(0, total); view.setUint32(4, headers.length); view.setUint32(8, crc32(bytes.subarray(0, 8)));
  bytes.set(headers, 12); bytes.set(message.payload, 12 + headers.length);
  view.setUint32(total - 4, crc32(bytes.subarray(0, total - 4)));
  return bytes;
}

function decodeHeaders(bytes: Uint8Array): EventMessage["headers"] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const headers: { name: string; value: EventHeaderValue }[] = [];
  let at = 0;
  const need = (count: number) => { if (at + count > bytes.length) throw frameError("a header runs past the headers section"); };
  while (at < bytes.length) {
    const length = bytes[at++];
    if (length === 0) throw frameError("a header name is empty");
    need(length);
    let name: string;
    try { name = text(bytes.subarray(at, at + length)); } catch { throw frameError("a header name is not UTF-8"); }
    at += length;
    need(1);
    const type = bytes[at++];
    let value: EventHeaderValue;
    switch (type) {
      case 0: case 1: value = { type: "boolean", value: type === 0 }; break;
      case 2: need(1); value = { type: "byte", value: view.getInt8(at) }; at += 1; break;
      case 3: need(2); value = { type: "short", value: view.getInt16(at) }; at += 2; break;
      case 4: need(4); value = { type: "integer", value: view.getInt32(at) }; at += 4; break;
      case 5: need(8); value = { type: "long", value: view.getBigInt64(at) }; at += 8; break;
      case 8: need(8); value = { type: "timestamp", value: Number(view.getBigInt64(at)) }; at += 8; break;
      case 9: need(16); value = { type: "uuid", value: bytes.slice(at, at + 16) }; at += 16; break;
      case 6: case 7: {
        need(2); const size = view.getUint16(at); at += 2; need(size);
        const raw = bytes.slice(at, at + size); at += size;
        if (type === 6) value = { type: "blob", value: raw };
        else { try { value = { type: "string", value: text(raw) }; } catch { throw frameError("a string header is not UTF-8"); } }
        break;
      }
      default: throw frameError(`unknown header value type ${type}`);
    }
    headers.push({ name, value });
  }
  return headers;
}

/** Exactly one complete message. */
export function decodeEventMessage(bytes: Uint8Array): EventMessage {
  const decoder = new EventMessageDecoder();
  decoder.push(bytes);
  const message = decoder.next();
  if (message === undefined) throw frameError("truncated message");
  if (!decoder.empty) throw frameError("bytes follow the message");
  return message;
}

/**
 * Reassembles messages from arbitrary chunks. Checksums are verified before
 * a message is returned, and the prelude's as soon as it arrives, so a
 * corrupt length never makes the decoder wait for data.
 */
export class EventMessageDecoder {
  #buffer = new Uint8Array(0);
  get empty(): boolean { return this.#buffer.length === 0; }
  push(chunk: Uint8Array): void {
    const next = new Uint8Array(this.#buffer.length + chunk.length);
    next.set(this.#buffer); next.set(chunk, this.#buffer.length);
    this.#buffer = next;
  }
  /** The next complete message, or undefined until more bytes arrive. */
  next(): EventMessage | undefined {
    const buffer = this.#buffer;
    if (buffer.length < 12) return undefined;
    const view = new DataView(buffer.buffer, buffer.byteOffset, buffer.byteLength);
    const total = view.getUint32(0), headers = view.getUint32(4);
    if (crc32(buffer.subarray(0, 8)) !== view.getUint32(8)) throw frameError("prelude checksum mismatch");
    if (headers > MAX_HEADERS) throw frameError("headers exceed 128 KiB");
    if (total < 16 + headers) throw frameError("the message is shorter than its headers");
    if (total - 16 - headers > MAX_PAYLOAD) throw frameError("payload exceeds 16 MiB");
    if (buffer.length < total) return undefined;
    if (crc32(buffer.subarray(0, total - 4)) !== view.getUint32(total - 4)) throw frameError("message checksum mismatch");
    const message = { headers: decodeHeaders(buffer.subarray(12, 12 + headers)), payload: buffer.slice(12 + headers, total - 4) };
    this.#buffer = buffer.slice(total);
    return message;
  }
}

function header(message: EventMessage, name: string): EventHeaderValue | undefined {
  return message.headers.find(item => item.name === name)?.value;
}
function stringHeader(message: EventMessage, name: string): string | undefined {
  const value = header(message, name);
  return value?.type === "string" ? value.value : undefined;
}

/** Supported modeled event-stream header kinds. */
export type EventHeaderKind = "boolean" | "byte" | "short" | "integer" | "long" | "blob" | "string" | "timestamp";

/** One union member: an event, or (with `error`) a modeled error the stream may end with. */
export interface EventSpec {
  /** The event structure's schema. */
  readonly schema?: Schema;
  /** The error table entry of an `@error` member. */
  readonly error?: string;
  /** Members bound to headers; the header is named for the member. */
  readonly headers?: readonly (readonly [member: string, kind: EventHeaderKind])[];
  /** The member bound to the payload; the others form a JSON document otherwise. */
  readonly payload?: readonly [member: string, kind: "blob" | "string" | "structure"];
}

/** An input or output member that carries an event stream. */
export interface EventStreamSpec {
  readonly member: string;
  /** Protocols without HTTP bindings send the other members as an initial-request/initial-response event. */
  readonly initial?: boolean;
  /** By union member name, which is each event's `:event-type`. */
  readonly events: Readonly<Record<string, EventSpec>>;
}

const string = (value: string): EventHeaderValue => ({ type: "string", value });
const RANGES = { byte: 8, short: 16, integer: 32 } as const;

function headerValue(kind: EventHeaderKind, value: unknown): EventHeaderValue {
  switch (kind) {
    case "boolean": if (typeof value !== "boolean") break; return { type: "boolean", value };
    case "byte": case "short": case "integer": {
      const limit = 2 ** (RANGES[kind] - 1);
      if (typeof value !== "number" || !Number.isInteger(value) || value < -limit || value >= limit) break;
      return { type: kind, value };
    }
    case "long":
      if (typeof value !== "bigint" || value < -(1n << 63n) || value >= 1n << 63n) break;
      return { type: "long", value };
    case "blob": if (!(value instanceof Uint8Array)) break; return { type: "blob", value };
    case "string": if (typeof value !== "string") break; return { type: "string", value };
    case "timestamp":
      if (!(value instanceof Date) || Number.isNaN(value.getTime())) break;
      return { type: "timestamp", value: value.getTime() };
  }
  throw new TypeError(`an event header of type ${kind} cannot hold this value`);
}

/** The header's value in the wire form `fromWire` reads. */
function wireHeader(kind: EventHeaderKind, value: EventHeaderValue): unknown {
  const integer = value.type === "byte" || value.type === "short" || value.type === "integer" || value.type === "long";
  switch (kind) {
    case "boolean": if (value.type === "boolean") return value.value; break;
    case "byte": case "short": case "integer": case "long":
      if (integer) return kind === "long" ? BigInt(value.value) : Number(value.value);
      break;
    case "blob": if (value.type === "blob") return toBase64(value.value); break;
    case "string": if (value.type === "string") return value.value; break;
    case "timestamp": if (value.type === "timestamp") return new Date(value.value).toISOString(); break;
  }
  throw new TypeError(`event header has type ${value.type}, not ${kind}`);
}

/** The message carrying one union value. */
export function encodeEvent(spec: EventStreamSpec, event: unknown): EventMessage {
  if (event === null || typeof event !== "object") throw new TypeError("an event is a union value");
  const names = Object.keys(event).filter(name => (event as Record<string, unknown>)[name] !== undefined);
  if (names.length !== 1) throw new TypeError("an event has exactly one member");
  const [name] = names;
  if (name === "$unknown" || !Object.hasOwn(spec.events, name)) throw new TypeError(`\`${name}\` is not an event of this stream`);
  const info = spec.events[name];
  const value = (event as Record<string, unknown>)[name] as Record<string, unknown>;
  // The whole structure is checked and converted first, so required header
  // members are validated too.
  const wire = toWire(info.schema, value ?? {}) as Record<string, unknown> | undefined ?? {};
  const headers: { name: string; value: EventHeaderValue }[] = info.error
    ? [{ name: ":message-type", value: string("exception") }, { name: ":exception-type", value: string(name) }]
    : [{ name: ":message-type", value: string("event") }, { name: ":event-type", value: string(name) }];
  const bound: { name: string; value: EventHeaderValue }[] = [];
  for (const [member, kind] of info.headers ?? []) {
    delete wire[wireName(info.schema, member)];
    const item = value?.[member];
    if (item !== undefined && item !== null) bound.push({ name: member, value: headerValue(kind, item) });
  }
  let contentType = "application/json";
  let payload: Uint8Array;
  if (info.payload) {
    const [member, kind] = info.payload;
    const item = value?.[member];
    if (kind === "blob") {
      contentType = "application/octet-stream";
      if (item !== undefined && item !== null && !(item instanceof Uint8Array)) throw new TypeError("a blob payload is a Uint8Array");
      payload = item instanceof Uint8Array ? item : new Uint8Array(0);
    } else if (kind === "string") {
      contentType = "text/plain";
      if (item !== undefined && item !== null && typeof item !== "string") throw new TypeError("a string payload is a string");
      payload = utf8.encode(typeof item === "string" ? item : "");
    } else {
      const document = wire[wireName(info.schema, member)];
      payload = document === undefined || document === null ? new Uint8Array(0) : utf8.encode(stringifyJson(document));
    }
  } else {
    payload = utf8.encode(stringifyJson(wire));
  }
  return { headers: [...headers, { name: ":content-type", value: string(contentType) }, ...bound], payload };
}

/** The `initial-request` message: the input's other members, as the document the body would have been. */
export function initialRequest(document: string): EventMessage {
  return {
    headers: [{ name: ":message-type", value: string("event") }, { name: ":event-type", value: string("initial-request") }, { name: ":content-type", value: string("application/json") }],
    payload: utf8.encode(document),
  };
}

/** Decoded event, modeled exception or unmodeled stream failure. */
export type Received =
  | { readonly kind: "event"; readonly value: unknown }
  | { readonly kind: "initial"; readonly payload: Uint8Array }
  | { readonly kind: "exception"; readonly name: string; readonly fields?: Record<string, unknown>; readonly message: string }
  | { readonly kind: "error"; readonly code: string; readonly message: string };

function fields(info: EventSpec, message: EventMessage): Record<string, unknown> {
  let result: Record<string, unknown>;
  if (info.payload) {
    const [member, kind] = info.payload;
    result = Object.create(null);
    const key = wireName(info.schema, member);
    if (kind === "blob") result[key] = toBase64(message.payload);
    else if (kind === "string") result[key] = text(message.payload);
    else if (message.payload.length > 0) result[key] = parseJson(text(message.payload));
  } else {
    const parsed = message.payload.length === 0 ? {} : parseJson(text(message.payload));
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) throw new TypeError("an event payload is not a JSON object");
    result = parsed as Record<string, unknown>;
  }
  for (const [member, kind] of info.headers ?? []) {
    const value = header(message, member);
    if (value !== undefined) result[wireName(info.schema, member)] = wireHeader(kind, value);
  }
  return result;
}

/** What a received message is. */
export function decodeEvent(spec: EventStreamSpec, message: EventMessage): Received {
  const type = stringHeader(message, ":message-type");
  if (type === "event") {
    const name = stringHeader(message, ":event-type");
    if (name === undefined) throw new TypeError("an event message has no :event-type");
    if (name === "initial-response" || name === "initial-request") return { kind: "initial", payload: message.payload };
    const info = Object.hasOwn(spec.events, name) ? spec.events[name] : undefined;
    if (info === undefined || info.error) {
      // A newer service's event: the union's unknown member.
      let json: string;
      try { json = stringifyJson(parseJson(text(message.payload))); } catch { json = JSON.stringify(toBase64(message.payload)); }
      return { kind: "event", value: { $unknown: { name, json } } };
    }
    return { kind: "event", value: { [name]: fromWire(info.schema, fields(info, message)) } };
  }
  if (type === "exception") {
    const name = stringHeader(message, ":exception-type");
    if (name === undefined) throw new TypeError("an exception message has no :exception-type");
    const info = Object.hasOwn(spec.events, name) ? spec.events[name] : undefined;
    let body: Record<string, unknown> = {};
    try { const parsed = parseJson(text(message.payload)); if (parsed !== null && typeof parsed === "object") body = parsed as Record<string, unknown>; } catch { /* not JSON */ }
    const messageText = typeof body.message === "string" ? body.message : typeof body.Message === "string" ? body.Message : "";
    if (info?.error) {
      let decoded: Record<string, unknown> | undefined;
      try { decoded = fields(info, message); } catch { /* retain the service failure when its body is malformed */ }
      return { kind: "exception", name: info.error, fields: decoded, message: messageText };
    }
    return { kind: "exception", name, fields: body, message: messageText };
  }
  if (type === "error") return { kind: "error", code: stringHeader(message, ":error-code") ?? "", message: stringHeader(message, ":error-message") ?? "" };
  throw new TypeError(type === undefined ? "a message has no :message-type" : `unknown :message-type ${type}`);
}

/** Messages from a byte stream, decoded as they complete. */
export class EventMessageReader {
  readonly #reader: ReadableStreamDefaultReader<Uint8Array>;
  readonly #decoder = new EventMessageDecoder();
  constructor(source: ReadableStream<Uint8Array>) { this.#reader = source.getReader(); }
  async next(): Promise<EventMessage | undefined> {
    for (;;) {
      const message = this.#decoder.next();
      if (message !== undefined) return message;
      const chunk = await this.#reader.read();
      if (chunk.done) {
        if (!this.#decoder.empty) throw frameError("the body ended inside a message");
        return undefined;
      }
      this.#decoder.push(chunk.value);
    }
  }
  async cancel(reason?: unknown): Promise<void> { await this.#reader.cancel(reason).catch(() => {}); }
}

/**
 * A one-shot stream of events. To send, construct one from an iterable or
 * async iterable of union values (an async generator, for example); events
 * are pulled only as the connection takes them. A received one is read
 * once with `for await`; leaving the loop early, or `cancel()`, releases
 * the connection. A modeled exception is thrown as its error class, an
 * unmodeled one as `EventStreamError`.
 */
export class EventStream<T> implements AsyncIterable<T> {
  readonly #source: AsyncIterable<T> | Iterable<T>;
  #taken = false;
  #iterator?: AsyncIterator<T>;
  constructor(events: AsyncIterable<T> | Iterable<T>) {
    if (events === null || typeof events !== "object" || (!(Symbol.asyncIterator in events) && !(Symbol.iterator in events))) throw new TypeError("EventStream requires an iterable or async iterable of events");
    this.#source = events;
  }
  [Symbol.asyncIterator](): AsyncIterator<T> {
    if (this.#taken) throw new TypeError("an event stream is read once");
    this.#taken = true;
    const source = this.#source;
    this.#iterator = Symbol.asyncIterator in source ? source[Symbol.asyncIterator]() : fromSync(source[Symbol.iterator]());
    return this.#iterator;
  }
  /** Stops reading: releases a received stream's connection, or ends a source. */
  async cancel(reason?: unknown): Promise<void> {
    if (!this.#taken) { this.#taken = true; await releases.get(this)?.(reason); return; }
    await this.#iterator?.return?.();
  }
}

async function* fromSync<T>(iterator: Iterator<T>): AsyncGenerator<T> {
  try { for (let next = iterator.next(); !next.done; next = iterator.next()) yield next.value; }
  finally { iterator.return?.(); }
}

const releases = new WeakMap<EventStream<unknown>, (reason?: unknown) => Promise<void>>();

/** For the transport: the events of a response, typed. */
export function receivedEvents<T>(spec: EventStreamSpec, reader: EventMessageReader, pending: EventMessage | undefined, errors: Readonly<Record<string, ErrorSpec>>, metadata: ResponseMetadata): EventStream<T> {
  async function* events(): AsyncGenerator<T> {
    let next = pending;
    try {
      for (;;) {
        const message = next ?? await reader.next();
        next = undefined;
        if (message === undefined) return;
        const received = decodeEvent(spec, message);
        if (received.kind === "event") { yield received.value as T; continue; }
        if (received.kind === "initial") continue;
        if (received.kind === "error") throw new EventStreamError(received.code, received.message);
        const error = Object.hasOwn(errors, received.name) ? errors[received.name] : undefined;
        let fields: Record<string, unknown> | undefined;
        if (received.fields !== undefined) {
          try { fields = fromWire(error?.schema, received.fields) as Record<string, unknown>; } catch { fields = undefined; }
        }
        if (error === undefined || fields === undefined) throw new ServiceException(received.name, received.message, metadata, error?.fault ?? "client", error?.retryable);
        throw error.create(fields, metadata);
      }
    } finally {
      await reader.cancel();
    }
  }
  const stream = new EventStream<T>(events());
  releases.set(stream as EventStream<unknown>, reason => reader.cancel(reason));
  return stream;
}

const hex = (bytes: ArrayBuffer | Uint8Array) => Array.from(bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes), b => b.toString(16).padStart(2, "0")).join("");
const sha256 = async (bytes: Uint8Array) => hex(await crypto.subtle.digest("SHA-256", bytes as BufferSource));
async function hmac(key: BufferSource, value: string): Promise<ArrayBuffer> {
  const imported = await crypto.subtle.importKey("raw", key, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return crypto.subtle.sign("HMAC", imported, utf8.encode(value) as BufferSource);
}

/** The payload hash a request declares when each event is signed as it is sent. */
export const STREAMING_EVENTS = "STREAMING-AWS4-HMAC-SHA256-EVENTS";

/** Signs event messages in sequence, each chaining the previous signature, from the request's own. */
export class EventSigner {
  #prior: string;
  constructor(readonly credentials: AwsCredentials, readonly scope: SigningScope, seed: string) { this.#prior = seed; }
  get prior(): string { return this.#prior; }
  async stringToSign(payload: Uint8Array, seconds: number): Promise<string> {
    const date = new Date(seconds * 1000).toISOString().replace(/[:-]|\.\d{3}/g, "");
    const header = encodeHeaders([{ name: ":date", value: { type: "timestamp", value: seconds * 1000 } }]);
    return `AWS4-HMAC-SHA256-PAYLOAD\n${date}\n${date.slice(0, 8)}/${this.scope.region}/${this.scope.name}/aws4_request\n${this.#prior}\n${await sha256(header)}\n${await sha256(payload)}`;
  }
  /** The signed envelope around `payload` (an encoded message, or nothing to end the stream). */
  async sign(payload: Uint8Array, now = Date.now()): Promise<EventMessage> {
    const seconds = Math.floor(now / 1000);
    const day = new Date(seconds * 1000).toISOString().slice(0, 10).replace(/-/g, "");
    let key = await hmac(utf8.encode(`AWS4${this.credentials.secretAccessKey}`) as BufferSource, day);
    key = await hmac(key, this.scope.region); key = await hmac(key, this.scope.name); key = await hmac(key, "aws4_request");
    const signature = new Uint8Array(await hmac(key, await this.stringToSign(payload, seconds)));
    this.#prior = hex(signature);
    return { headers: [{ name: ":chunk-signature", value: { type: "blob", value: signature } }, { name: ":date", value: { type: "timestamp", value: seconds * 1000 } }], payload };
  }
}

/** Resolved once the request is signed (with its signer) or known to be unsigned. */
export interface EventSigning {
  readonly ready: Promise<EventSigner | undefined>;
  resolve(signer: EventSigner | undefined): void;
}
/** Create the promise that releases event framing once signing credentials are resolved. */
export function eventSigning(): EventSigning {
  let resolve!: (signer: EventSigner | undefined) => void;
  const ready = new Promise<EventSigner | undefined>(done => { resolve = done; });
  return { ready, resolve };
}

/**
 * The request body: the initial request first when the protocol has one,
 * then one message per event, pulled from the caller's source only when the
 * connection asks for more. A signed stream ends with an empty signed message.
 */
export function eventBody<T>(spec: EventStreamSpec, events: AsyncIterable<T> | Iterable<T>, initial: string | undefined, signing: EventSigning): ReadableStream<Uint8Array> {
  const source = events instanceof EventStream ? events[Symbol.asyncIterator]() : new EventStream(events)[Symbol.asyncIterator]();
  let pendingInitial = initial;
  let finished = false;
  const frame = async (message: EventMessage, signer: EventSigner | undefined) => {
    const bytes = encodeEventMessage(message);
    return signer ? encodeEventMessage(await signer.sign(bytes)) : bytes;
  };
  return new ReadableStream<Uint8Array>({
    async pull(controller) {
      const signer = await signing.ready;
      if (pendingInitial !== undefined) {
        const document = pendingInitial; pendingInitial = undefined;
        controller.enqueue(await frame(initialRequest(document), signer)); return;
      }
      const next = await source.next();
      if (next.done) {
        finished = true;
        if (signer) controller.enqueue(encodeEventMessage(await signer.sign(new Uint8Array(0))));
        controller.close(); return;
      }
      controller.enqueue(await frame(encodeEvent(spec, next.value), signer));
    },
    async cancel() { if (!finished) await source.return?.(); },
  }, { highWaterMark: 0 });
}
