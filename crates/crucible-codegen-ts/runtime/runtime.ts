// crucible TypeScript runtime. Generated clients carry a copy of this file
// and its companion modules, with no external runtime dependencies.
// Edit it in the crucible repository (crates/crucible-emit-ts/runtime/), never
// in a generated package.

import { hostPrefix, parseEndpoint, resolveEndpoint, type EndpointPrefix, type EndpointRules, type ParameterSource } from "./endpoints.js";
import { waitFor, type WaiterSpec, type WaiterOptions, type WaiterResult } from "./waiters.js";
import { signSigV4, type AwsCredentials, type SigningScope } from "./credentials.js";
import { ChecksumError, computeChecksum, planChecksum, responseChecksum, sameChecksum, verifyingStream, type ChecksumPlan, type ChecksumPolicy, type ChecksumSpec } from "./checksums.js";
import { EVENT_STREAM_CONTENT_TYPE, EventMessageReader, EventSigner, EventStream, STREAMING_EVENTS, decodeEvent, eventBody, eventSigning, receivedEvents, type EventMessage, type EventSigning, type EventStreamSpec } from "./events.js";
export { EventStream, EventStreamError } from "./events.js";
export type { EventStreamSpec } from "./events.js";
type Signing = {credentials: AwsCredentials; scope: SigningScope; streaming: boolean};
const signingRequests = new WeakMap<Request, Signing>();
export type { WaiterSpec } from "./waiters.js";
export type { EndpointRules } from "./endpoints.js";

/** Any JSON value: what a Smithy `document` holds. */
export type Document = null | boolean | number | string | Document[] | { [key: string]: Document };

/** An exact decimal JSON number. Construct from text to avoid floating-point rounding. */
export class Decimal {
  readonly #text: string;
  constructor(value: string | bigint) {
    const text = String(value);
    if (!/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/.test(text)) throw new TypeError("invalid decimal");
    this.#text = text;
  }
  toString(): string { return this.#text; }
  equals(other: Decimal): boolean { return numericKey(this.#text) === numericKey(other.#text); }
}
function numericKey(text: string): string {
  const match = /^(-?)(\d+)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/.exec(text)!;
  let digits = (match[2] + (match[3] ?? "")).replace(/^0+/, "");
  if (!digits) return "0";
  let exponent = BigInt(match[4] ?? "0") - BigInt(match[3]?.length ?? 0);
  const zeroes = /0*$/.exec(digits)![0].length;
  digits = digits.slice(0, digits.length - zeroes); exponent += BigInt(zeroes);
  return `${match[1]}${digits}e${exponent}`;
}

/** A streaming payload. Factory bodies must reproduce the same bytes on every retry. */
export class ByteStream {
  readonly replayable: boolean;
  readonly length?: number;
  readonly sha256?: string;
  #source: ReadableStream<Uint8Array> | (() => ReadableStream<Uint8Array>);
  #opened = false;
  constructor(source: ReadableStream<Uint8Array> | (() => ReadableStream<Uint8Array>), options: { length?: number; sha256?: string } = {}) {
    if (!(source instanceof ReadableStream) && typeof source !== "function") throw new TypeError("ByteStream requires a ReadableStream or replay factory");
    if (options.length !== undefined && (!Number.isSafeInteger(options.length) || options.length < 0)) throw new TypeError("stream length must be a nonnegative safe integer");
    if (options.sha256 !== undefined && !/^[a-fA-F0-9]{64}$/.test(options.sha256)) throw new TypeError("sha256 must contain 64 hexadecimal digits");
    this.#source = source; this.replayable = typeof source === "function";
    this.length = options.length; this.sha256 = options.sha256?.toLowerCase();
  }
  /** Open once, or once per attempt for a replay factory. Read with getReader(). */
  open(): ReadableStream<Uint8Array> {
    if (this.#opened && !this.replayable) throw new TypeError("one-shot ByteStream already opened");
    this.#opened = true;
    const source = typeof this.#source === "function" ? this.#source() : this.#source;
    if (!(source instanceof ReadableStream)) throw new TypeError("stream factory must return a ReadableStream");
    return source;
  }
  /** Release an unopened response body without reading it. */
  async cancel(reason?: unknown): Promise<void> {
    if (this.#opened) throw new TypeError("cancel the opened stream or reader");
    this.#opened = true;
    if (typeof this.#source !== "function") await this.#source.cancel(reason);
  }
}

/** A future union arm. `json` is its lossless JSON payload, not the enclosing union. */
export interface UnknownUnion {
  readonly $unknown: { readonly name: string; readonly json: string };
}

/** A union member or result that carries no value. On the wire it is `{}`. */
export type Unit = Record<string, never>;

export type TimestampFormat = "date-time" | "http-date" | "epoch-seconds";

/** Static wire conversion and validation metadata emitted from the Smithy IR. */
export type Schema =
  | { readonly kind: "string" | "boolean" | "document" | "blob" | "bigInteger" | "bigDecimal" }
  | { readonly kind: "number"; readonly nonFinite?: boolean }
  | { readonly kind: "integer"; readonly bits: 8 | 16 | 32 | 64 }
  | { readonly kind: "timestamp"; readonly format: TimestampFormat }
  | { readonly kind: "list"; readonly member: Schema; readonly sparse?: boolean; readonly unique?: boolean }
  | { readonly kind: "map"; readonly value: Schema; readonly sparse?: boolean }
  | {
      readonly kind: "structure" | "union";
      readonly members: Readonly<Record<string, Schema>>;
      readonly defaults?: Readonly<Record<string, unknown>>;
      /** Nonoptional client members, settled by the IR (not inferred here). */
      readonly required?: readonly string[];
      readonly jsonNames?: Readonly<Record<string, string>>;
      readonly modelNames?: Readonly<Record<string, string>>;
      readonly omitNull?: boolean;
      readonly ignoreType?: boolean;
    };

export const string: Schema = { kind: "string" };
export const number: Schema = { kind: "number" };
export const boolean: Schema = { kind: "boolean" };
export const document: Schema = { kind: "document" };
export const blob: Schema = { kind: "blob" };
export const bigInteger: Schema = { kind: "bigInteger" };
export const bigDecimal: Schema = { kind: "bigDecimal" };
export const integer = (bits: 8 | 16 | 32 | 64): Schema => ({ kind: "integer", bits });
export const timestamp = (format: TimestampFormat): Schema => ({ kind: "timestamp", format });
export function list(member: Schema | (() => Schema), options: { sparse?: boolean; unique?: boolean } = {}): Schema {
  return { kind: "list", ...options, get member() { return typeof member === "function" ? member() : member; } };
}
export function map(value: Schema | (() => Schema), options: { sparse?: boolean } = {}): Schema {
  return { kind: "map", ...options, get value() { return typeof value === "function" ? value() : value; } };
}

/** Thunks also support recursive list/map aliases without declaration-order work. */
export function structure(members: () => Record<string, Schema>, defaults?: Record<string, unknown>, required?: readonly string[], jsonNames?: Readonly<Record<string, string>>, omitNull=false): Schema {
  let resolved: Record<string, Schema> | undefined;
  return { kind: "structure", get members() { return (resolved ??= members()); }, defaults, required, jsonNames, modelNames: reverseNames(jsonNames), omitNull };
}
export function union(members: () => Record<string, Schema>, jsonNames?: Readonly<Record<string, string>>, ignoreType=false): Schema {
  let resolved: Record<string, Schema> | undefined;
  return { kind: "union", get members() { return (resolved ??= members()); }, jsonNames, modelNames: reverseNames(jsonNames), ignoreType };
}

function reverseNames(names: Readonly<Record<string, string>> | undefined): Readonly<Record<string, string>> {
  return Object.fromEntries(Object.entries(names ?? {}).map(([model, wire]) => [wire, model]));
}
export function wireName(schema: Schema | undefined, name: string): string {
  if (schema?.kind !== "structure" && schema?.kind !== "union") return name;
  return schema.jsonNames && Object.hasOwn(schema.jsonNames, name) ? schema.jsonNames[name] : name;
}

/** An internal numeric token. It never escapes decoding into a public model. */
class JsonNumber {
  constructor(readonly text: string) {}
}

function exactInteger(value: unknown, bits: 8 | 16 | 32 | 64 | undefined, encoding: boolean): bigint {
  if (encoding && ((bits === 64 || bits === undefined) ? typeof value !== "bigint" : typeof value !== "number")) {
    throw new TypeError((bits === 64 || bits === undefined) ? "an exact integer is a bigint" : "an integer is a number");
  }
  if (typeof value === "number" && !Number.isSafeInteger(value)) {
    throw new TypeError("integer requires an exact numeric token or a safe integer");
  }
  if (!(value instanceof JsonNumber) && typeof value !== "number" && typeof value !== "bigint") {
    throw new TypeError("expected integer");
  }
  const text = value instanceof JsonNumber ? value.text : String(value);
  const match = /^(-?)(\d+)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/.exec(text);
  if (!match) throw new TypeError("invalid integer");
  let digits = (match[2] + (match[3] ?? "")).replace(/^0+/, "");
  if (digits.length === 0) return 0n;
  const shift = Number(match[4] ?? "0") - (match[3]?.length ?? 0);
  if (!Number.isSafeInteger(shift)) throw new TypeError("integer outside range");
  if (shift < 0) {
    if (-shift >= digits.length || !/^0*$/.test(digits.slice(shift))) throw new TypeError("fractional integer");
    digits = digits.slice(0, shift);
  } else {
    if (bits !== undefined && digits.length + shift > 19) throw new TypeError("integer outside range");
    digits += "0".repeat(shift);
  }
  if (bits !== undefined && digits.length > 19) throw new TypeError("integer outside range");
  const result = BigInt(match[1] + digits);
  if (bits === undefined) return result;
  const maximum = 1n << BigInt(bits - 1);
  if (result < -maximum || result >= maximum) throw new TypeError(`integer outside signed ${bits}-bit range`);
  return result;
}

/** Restore the documented JavaScript number representation of JSON documents. */
function jsonValue(value: unknown): unknown {
  if (value instanceof JsonNumber) return jsonValue(Number(value.text));
  if (typeof value === "number" && !Number.isFinite(value)) throw new TypeError("non-finite JSON number");
  if (value instanceof Uint8Array || value instanceof Date || value instanceof ByteStream || value instanceof Decimal || value instanceof EventStream) return value;
  if (Array.isArray(value)) return value.map(jsonValue);
  if (value !== null && typeof value === "object") return mapValues(value as Record<string, unknown>, jsonValue);
  return value;
}
function object(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value) || value instanceof JsonNumber) {
    throw new TypeError("expected object");
  }
  return value as Record<string, unknown>;
}

export function toWire(schema: Schema | undefined, value: unknown): unknown {
  if (schema === undefined || value === undefined || value === null) return value;
  switch (schema.kind) {
    case "bigInteger": return exactInteger(value, undefined, true);
    case "bigDecimal":
      if (!(value instanceof Decimal)) throw new TypeError("a bigDecimal is a Decimal");
      return new JsonNumber(value.toString());
    case "integer": {
      const result = exactInteger(value, schema.bits, true);
      return schema.bits === 64 ? result : Number(result);
    }
    case "number":
      if (typeof value !== "number") throw new TypeError("expected number");
      if (schema.nonFinite && !Number.isFinite(value)) return Number.isNaN(value)?"NaN":value>0?"Infinity":"-Infinity";
      if (!Number.isFinite(value)) throw new TypeError("expected finite number");
      return value;
    case "string": case "boolean":
      if (typeof value !== schema.kind || (typeof value === "number" && !Number.isFinite(value))) throw new TypeError(`expected ${schema.kind}`);
      return value;
    case "timestamp": return formatTimestamp(value as Date, schema.format);
    case "blob": return toBase64(value as Uint8Array);
    case "list":
      if (!Array.isArray(value)) throw new TypeError("expected array");
      return convertList(schema, value, toWire);
    case "map": return mapValues(object(value), item => collectionValue(schema.value, item, toWire, schema.sparse));
    case "structure": case "union": return convertMembers(schema, object(value), toWire);
    default: return value;
  }
}

export function fromWire(schema: Schema | undefined, value: unknown): unknown {
  if (schema === undefined || schema.kind === "document" || value === undefined || value === null) return jsonValue(value);
  switch (schema.kind) {
    case "bigInteger": return exactInteger(value, undefined, false);
    case "bigDecimal":
      if (!(value instanceof JsonNumber) && typeof value !== "number" && typeof value !== "bigint") throw new TypeError("expected decimal number");
      return new Decimal(value instanceof JsonNumber ? value.text : String(value));
    case "integer": {
      const result = exactInteger(value, schema.bits, false);
      return schema.bits === 64 ? result : Number(result);
    }
    case "number": {
      if (schema.nonFinite && ["NaN","Infinity","-Infinity"].includes(value as string)) return Number(value);
      const result = value instanceof JsonNumber ? Number(value.text) : value;
      if (typeof result !== "number" || !Number.isFinite(result)) throw new TypeError("expected finite number");
      return result;
    }
    case "string": case "boolean":
      if (typeof value !== schema.kind) throw new TypeError(`expected ${schema.kind}`);
      return value;
    case "timestamp": return parseTimestamp((value instanceof JsonNumber ? Number(value.text) : value) as string | number);
    case "blob": return fromBase64(value as string);
    case "list":
      if (!Array.isArray(value)) throw new TypeError("expected array");
      return convertList(schema, value, fromWire);
    case "map": return mapValues(object(value), item => collectionValue(schema.value, item, fromWire, schema.sparse));
    case "structure": case "union": return convertMembers(schema, object(value), fromWire);
  }
}

function collectionValue(schema: Schema, value: unknown, convert: typeof toWire, sparse = false): unknown {
  if (value === undefined || (value === null && !sparse)) {
    throw new TypeError("dense collection members cannot be null or undefined");
  }
  return convert(schema, value);
}

function convertList(schema: Extract<Schema, { kind: "list" }>, value: unknown[], convert: typeof toWire): unknown[] {
  const result = Array.from(value, item => collectionValue(schema.member, item, convert, schema.sparse));
  if (schema.unique) {
    const keys = result.map(item => valueKey(convert === toWire ? item : toWire(schema.member, item)));
    if (new Set(keys).size !== keys.length) throw new TypeError("set members must be unique by value");
  }
  return result;
}
function valueKey(value: unknown): string {
  if (value instanceof JsonNumber) return "n:" + numericKey(value.text);
  if (typeof value === "number" || typeof value === "bigint") return "n:" + numericKey(String(value));
  if (Array.isArray(value)) return "[" + value.map(valueKey).join(",") + "]";
  if (value !== null && typeof value === "object") return "{" + Object.keys(value).sort().map(key => JSON.stringify(key) + ":" + valueKey((value as Record<string, unknown>)[key])).join(",") + "}";
  return JSON.stringify(value);
}
/** Clone public values while keeping exact decimals and streaming body ownership. */
function snapshotValue<T>(value: T, seen = new Map<object, unknown>()): T {
  if (value === null || typeof value !== "object") return value;
  if (value instanceof ByteStream || value instanceof Decimal || value instanceof EventStream) return value;
  if (value instanceof Date) return new Date(value.getTime()) as T;
  if (value instanceof Uint8Array) return value.slice() as T;
  if (seen.has(value)) return seen.get(value) as T;
  const result = (Array.isArray(value) ? new Array(value.length) : Object.create(null)) as Record<string, unknown>;
  seen.set(value, result);
  for (const [key, item] of Object.entries(value)) result[key] = snapshotValue(item, seen);
  return result as T;
}

function convertMembers(
  schema: Extract<Schema, { kind: "structure" | "union" }>,
  value: Record<string, unknown>,
  convert: (schema: Schema | undefined, value: unknown) => unknown,
): Record<string, unknown> {
  const names = Object.keys(value).filter(name => value[name] !== undefined && !(schema.ignoreType && convert===fromWire && name==="__type"));
  if (schema.kind === "union") {
    if (names.length !== 1) throw new TypeError("a union must have exactly one member");
    const name = names[0];
    if (convert === toWire && name === "$unknown") {
      const arm = object(value.$unknown);
      if (typeof arm.name !== "string" || typeof arm.json !== "string" ||
          Object.keys(schema.members).some(member => wireName(schema, member) === arm.name)) {
        throw new TypeError("unknown union arm must name an undeclared member and carry JSON text");
      }
      return Object.fromEntries([[arm.name, parseJson(arm.json)]]);
    }
    const model = convert === fromWire && schema.modelNames && Object.hasOwn(schema.modelNames, name) ? schema.modelNames[name] : name;
    if (!Object.hasOwn(schema.members, model) || (convert === fromWire && wireName(schema, model) !== name)) {
      if (convert === toWire) throw new TypeError("undeclared union arm requires $unknown");
      return { $unknown: { name, json: stringifyJson(value[name]) } };
    }
    return Object.fromEntries([[convert === toWire ? wireName(schema, model) : model, convert(schema.members[model], value[name])]]);
  }
  const native: Record<string, unknown> = Object.create(null);
  for (const name of Object.keys(value)) {
    const model = convert === fromWire && schema.modelNames && Object.hasOwn(schema.modelNames, name) ? schema.modelNames[name] : name;
    if (schema.omitNull && value[name]===null && schema.members[model]?.kind!=="document") continue;
    // A stale wire property must not masquerade as a renamed modeled member.
    if (convert === fromWire && Object.hasOwn(schema.members, model) && wireName(schema, model) !== name) continue;
    native[model] = value[name];
  }
  if (convert === fromWire) {
    for (const [name, fallback] of Object.entries(schema.defaults ?? {})) {
      if (!Object.hasOwn(native, name) || native[name] === undefined) native[name] = fallback;
    }
  }
  for (const name of schema.required ?? []) {
    if (!Object.hasOwn(native, name) || native[name] === undefined) throw new TypeError(`missing required member: ${name}`);
    if (native[name] === null && schema.members[name]?.kind !== "document") throw new TypeError(`null required member: ${name}`);
  }
  return Object.fromEntries(Object.keys(native).map(name => {
    const member = Object.hasOwn(schema.members, name) ? schema.members[name] : undefined;
    return [convert === toWire && member !== undefined ? wireName(schema, name) : name, convert(member, native[name])];
  }));
}

function mapValues(value: Record<string, unknown>, convert: (value: unknown) => unknown): Record<string, unknown> {
  return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, convert(item)]));
}

/** Parse JSON retaining numeric source tokens until a schema chooses their type. */
export function parseJson(text: string): unknown {
  // The platform parser owns JSON grammar and string escape validation. A second
  // token pass preserves numbers without depending on ES2024 reviver contexts.
  JSON.parse(text);
  const tokens = text.match(/"(?:[^"\\]|\\.)*"|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?|true|false|null|[{}\[\],:]/g)!;
  let at = 0;
  const read = (depth: number): unknown => {
    if (depth > 255) throw new TypeError("JSON nesting exceeds 255 levels");
    const token = tokens[at++];
    if (token === "{") {
      const entries: [string, unknown][] = [];
      if (tokens[at] !== "}") do {
        const key = JSON.parse(tokens[at++]) as string;
        at++; // colon, already validated
        entries.push([key, read(depth + 1)]);
        if (tokens[at] !== ",") break;
        at++;
      } while (true);
      at++;
      return Object.fromEntries(entries);
    }
    if (token === "[") {
      const result: unknown[] = [];
      if (tokens[at] !== "]") do {
        result.push(read(depth + 1));
        if (tokens[at] !== ",") break;
        at++;
      } while (true);
      at++;
      return result;
    }
    return /^-?\d/.test(token) ? new JsonNumber(token) : JSON.parse(token);
  };
  return read(0);
}

/** Write converted model values, including exact unquoted bigint JSON numbers. */
export function stringifyJson(value: unknown): string {
  const active = new Set<object>();
  const write = (item: unknown, depth: number): string => {
    if (depth > 255) throw new TypeError("JSON nesting exceeds 255 levels");
    if (item instanceof JsonNumber) return item.text;
    if (item instanceof Decimal) return item.toString();
    if (typeof item === "bigint") return item.toString();
    if (item === null || typeof item !== "object") {
      if (typeof item === "number" && !Number.isFinite(item)) throw new TypeError("non-finite JSON number");
      const encoded = JSON.stringify(item);
      if (encoded === undefined) throw new TypeError("not a JSON value");
      return encoded;
    }
    if (!Array.isArray(item) && Object.getPrototypeOf(item) !== Object.prototype && Object.getPrototypeOf(item) !== null) throw new TypeError("expected plain JSON object");
    if (active.has(item)) throw new TypeError("circular JSON value");
    active.add(item);
    const result = Array.isArray(item)
      ? "[" + Array.from(item, value => write(value === undefined ? null : value, depth + 1)).join(",") + "]"
      : "{" + Object.entries(item).filter(([, value]) => value !== undefined)
        .map(([key, value]) => JSON.stringify(key) + ":" + write(value, depth + 1)).join(",") + "}";
    active.delete(item);
    return result;
  };
  return write(value, 0);
}

export function formatTimestamp(value: Date, format: TimestampFormat): string | number {
  switch (format) {
    case "date-time":
      return value.toISOString();
    case "http-date":
      return value.toUTCString();
    case "epoch-seconds":
      return value.getTime() / 1000;
  }
}

/** Standard, padded base64: how a JSON body carries a blob. */
export function toBase64(bytes: Uint8Array): string {
  if (!(bytes instanceof Uint8Array)) throw new TypeError("a blob is a Uint8Array");
  let binary = "";
  // Chunked, so a large blob does not overflow the argument list.
  for (let at = 0; at < bytes.length; at += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  }
  return btoa(binary);
}

export function fromBase64(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let at = 0; at < binary.length; at++) bytes[at] = binary.charCodeAt(at);
  return bytes;
}

// Seconds times 1000 can land just below the true millisecond (1090069807.626
// * 1000 is ...625.9999), so snap values within float noise to the nearest
// millisecond. Finer fractions truncate, as `Date` does for date-time text.
function epochMillis(seconds: number): number {
  const millis = seconds * 1000;
  const nearest = Math.round(millis);
  return Math.abs(millis - nearest) < 5e-4 ? nearest : Math.floor(millis);
}

export function parseTimestamp(value: string | number): Date {
  const date = typeof value === "number" ? new Date(epochMillis(value)) : new Date(value);
  if (Number.isNaN(date.getTime())) throw new TypeError(`not a timestamp: ${String(value)}`);
  return date;
}

/** Where each member of an operation's input and output travels. */
export interface OperationSpec {
  readonly protocol?: "crucibleJson" | "awsRestJson1" | "awsJson1_0" | "awsJson1_1";
  readonly target?: string;
  readonly headerEncodings?: Readonly<Record<string,"base64">>;
  readonly unsignedPayload?: boolean;
  /** Request and response checksums (`@httpChecksumRequired`, `@aws.protocols#httpChecksum`). */
  readonly checksum?: ChecksumSpec;
  readonly name: string;
  /** Effective operation authentication; omitted only by older hand-written specs. */
  readonly bearer?: boolean;
  readonly auth?: readonly AuthScheme[];
  readonly optionalAuth?: boolean;
  readonly endpoint?: EndpointPrefix;
  readonly waiters?: Readonly<Record<string, WaiterSpec>>;
  readonly endpointParameters?: Readonly<Record<string, readonly ParameterSource[]>>;
  readonly method: string;
  /** Literal path segments and `{ label: member }` placeholders. */
  readonly path: readonly (string | { readonly label: string; readonly schema?: Schema; readonly greedy?: boolean })[];
  readonly queryParams?: string;
  readonly prefixHeaders?: readonly (readonly [member: string, prefix: string])[];
  readonly literalQuery?: readonly (readonly [string, string | undefined])[];
  readonly query?: readonly (readonly [member: string, param: string, schema?: Schema])[];
  readonly headers?: readonly (readonly [member: string, header: string, schema?: Schema])[];
  /** Input members in the JSON body; absent when the request has no body. */
  readonly body?: readonly string[];
  /** The input member that is the whole body, raw (`@httpPayload`), and its content type. */
  readonly payload?: { readonly member: string; readonly mediaType: string; readonly kind?: "blob" | "string" | "document"; readonly streaming?: boolean; readonly requiresLength?: boolean; readonly minimum?: number; readonly maximum?: number };
  readonly input?: Schema;
  /** The input member that sends an event stream. */
  readonly inputEvents?: EventStreamSpec;
  readonly output?: {
    /** The output member that receives an event stream. */
    readonly events?: EventStreamSpec;
    readonly body: boolean;
    readonly responseCode?: string;
    readonly prefixHeaders?: readonly (readonly [member: string, prefix: string])[];
    /** The output member that is the whole body, read as bytes. */
    readonly payload?: string;
    readonly payloadKind?: "blob" | "string" | "document";
    readonly headerEncodings?: Readonly<Record<string,"base64">>;
    readonly streaming?: boolean;
    readonly minimum?: number;
    readonly maximum?: number;
    readonly schema?: Schema;
    readonly headers?: readonly (readonly [member: string, header: string, schema?: Schema])[];
  };
  /** The errors this operation can return, by shape name. */
  readonly errors: readonly string[];
  readonly idempotencyToken?: string;
  /** Repeating the request cannot repeat its effect: readonly, idempotent or tokened. */
  readonly retrySafe: boolean;
}

export interface ResponseMetadata {
  readonly status: number;
  readonly requestId?: string;
  readonly headers: Headers;
}

/** An error response the service sent. Each modelled error has its own subclass. */
export class ServiceError extends Error {
  readonly $metadata: ResponseMetadata;
  readonly $fault: "client" | "server";
  readonly $retryable?: { readonly throttling: boolean };

  constructor(
    name: string,
    message: string,
    metadata: ResponseMetadata,
    fault: "client" | "server",
    retryable?: { readonly throttling: boolean },
  ) {
    super(message);
    this.name = name;
    this.$metadata = metadata;
    this.$fault = fault;
    this.$retryable = retryable;
  }
}

/** Base for every error response from this service. */
export class ServiceException extends ServiceError {}

/** The request never got a response: DNS, connection, TLS or CORS. */
export class NetworkError extends Error {
  constructor(readonly cause: unknown) {
    super(cause instanceof Error ? cause.message : String(cause));
    this.name = "NetworkError";
  }
}

/** The configured total-call budget expired. */
export class TimeoutError extends Error {
  constructor() { super("call deadline exceeded"); this.name = "TimeoutError"; }
}

export interface ErrorSpec {
  readonly payload?: string;
  readonly payloadKind?: "blob" | "string" | "document";
  readonly headerEncodings?: Readonly<Record<string,"base64">>;
  readonly responseCode?: string;
  readonly prefixHeaders?: readonly (readonly [member: string, prefix: string])[];
  readonly status: number;
  readonly fault?: "client" | "server";
  readonly retryable?: { readonly throttling: boolean };
  readonly schema?: Schema;
  readonly headers?: readonly (readonly [member: string, header: string, schema?: Schema])[];
  readonly create: (fields: Record<string, unknown>, metadata: ResponseMetadata) => ServiceError;
}

export type Fetch = (input: Request) => Promise<Response>;
export type Next = (request: Request) => Promise<Response>;
/** Sees every attempt of every request, in order, before it is sent. */
export type Middleware = (request: Request, next: Next) => Promise<Response>;
export type IdentityProvider<T> = T | (() => T | undefined | Promise<T | undefined>);
export type TokenProvider = IdentityProvider<string>;
export interface BasicCredentials { readonly username: string; readonly password: string; }
export type AuthScheme = { readonly kind: "bearer" } | { readonly kind: "basic" } | { readonly kind: "apiKey"; readonly name: string; readonly location: "header" | "query"; readonly scheme?: string | null } | { readonly kind: "sigV4"; readonly signingName: string };
export class AuthenticationError extends Error {
  constructor(message: string) { super(message); this.name = "AuthenticationError"; }
}

export interface RetryConfig {
  /** Attempts in all, the first included. */
  readonly maxAttempts?: number;
  readonly baseDelayMs?: number;
  readonly maxDelayMs?: number;
}

export interface ClientConfig {
  readonly endpoint?: string;
  readonly region?: string;
  readonly endpointParameters?: Readonly<Record<string, unknown>>;
  readonly endpointBuiltins?: Readonly<Record<string, unknown>>;
  readonly disableHostPrefix?: boolean;
  /** Sent as a bearer token. A function is called for every attempt, so a refreshed token is picked up. */
  readonly token?: TokenProvider;
  readonly basicCredentials?: IdentityProvider<BasicCredentials>;
  readonly apiKey?: IdentityProvider<string>;
  readonly awsCredentials?: IdentityProvider<AwsCredentials>;
  readonly signingName?: string;
  readonly fetch?: Fetch;
  /** Headers sent on every request. */
  readonly headers?: Readonly<Record<string, string>>;
  /** Runs after the client's own middleware, nearest the network last. */
  readonly middleware?: readonly Middleware[];
  readonly retry?: RetryConfig | false;
  /** Total call budget, including credentials, middleware, attempts and response bodies. */
  readonly timeoutMs?: number;
  /** When modeled request checksums are computed. `WHEN_SUPPORTED` (the default) also sends CRC32 when the caller chose no algorithm. */
  readonly requestChecksumCalculation?: ChecksumPolicy;
  /** When modeled response checksums are verified. `WHEN_SUPPORTED` (the default) also asks the service for one. */
  readonly responseChecksumValidation?: ChecksumPolicy;
  /** Told the operation and algorithm of each verified response checksum; a streamed body is verified when it ends. */
  readonly checksumValidated?: (operation: string, algorithm: string) => void;
}

export interface RequestOptions {
  /** Overrides the client budget; null disables it for this call. */
  readonly timeoutMs?: number | null;
  readonly signal?: AbortSignal;
  readonly headers?: Readonly<Record<string, string>>;
}

const RETRYABLE_STATUS = new Set([500, 502, 503, 504]);

export class Transport {
  readonly #config: ClientConfig;
  readonly #endpoint: string;
  readonly #endpointRules?: EndpointRules;
  readonly #middleware: readonly Middleware[];
  readonly #bearer: boolean;
  readonly #errors: Readonly<Record<string, ErrorSpec>>;

  constructor(
    config: ClientConfig,
    defaults: { readonly endpoint?: string; readonly middleware: readonly Middleware[]; readonly bearer: boolean; readonly endpointRules?: EndpointRules },
    errors: Readonly<Record<string, ErrorSpec>>,
  ) {
    const endpoint = config.endpoint ?? defaults.endpoint;
    if (!endpoint && !defaults.endpointRules) throw new TypeError("no endpoint: pass `endpoint` in the client config");
    if (endpoint && !parseEndpoint(endpoint)) throw new TypeError("endpoint must be an absolute HTTP(S) URL without credentials, query, or fragment");
    for (const policy of [config.requestChecksumCalculation, config.responseChecksumValidation]) {
      if (policy !== undefined && policy !== "WHEN_SUPPORTED" && policy !== "WHEN_REQUIRED") throw new TypeError("checksum policies must be WHEN_SUPPORTED or WHEN_REQUIRED");
    }
    this.#config = config;
    this.#endpoint = endpoint?.replace(/\/+$/, "") ?? "";
    this.#endpointRules = defaults.endpointRules;
    this.#errors = errors;
    this.#bearer = defaults.bearer;
    this.#middleware = [...defaults.middleware, ...(config.middleware ?? [])];
  }

  wait<O>(operation: OperationSpec, name: string, input: object, options: WaiterOptions): Promise<WaiterResult<O>> {
    const spec = operation.waiters?.[name]; if (!spec) throw new TypeError(`unknown waiter ${name}`);
    const snapshot = snapshotValue(withIdempotencyToken(operation, input)) as object;
    return waitFor(name, spec, snapshot, (input, options) => this.send<O>(operation, input, options), options);
  }

  async send<O>(operation: OperationSpec, input: object, options: RequestOptions = {}): Promise<O> {
    const timeout = options.timeoutMs === undefined ? this.#config.timeoutMs : options.timeoutMs;
    if (timeout != null && (!Number.isFinite(timeout) || timeout < 0)) throw new TypeError("timeoutMs must be a finite nonnegative number");
    const controller = new AbortController();
    const abort = () => controller.abort(options.signal?.reason);
    if (options.signal?.aborted) abort();
    else options.signal?.addEventListener("abort", abort, { once: true });
    let timer: ReturnType<typeof setTimeout> | undefined;
    if (timeout != null) {
      const deadline = performance.now() + timeout;
      const arm = () => {
        const remaining = deadline - performance.now();
        if (remaining <= 0) controller.abort(new TimeoutError());
        else timer = setTimeout(arm, Math.min(2_147_483_647, remaining));
      };
      arm();
    }
    let retained = false;
    const cleanup = () => {
      if (timer !== undefined) clearTimeout(timer);
      options.signal?.removeEventListener("abort", abort);
    };
    try {
      controller.signal.throwIfAborted();
      const result = await abortable(this.#send<O>(operation, input, { ...options, signal: controller.signal }), controller.signal);
      const events = operation.output?.events && (result as Record<string, unknown>)[operation.output.events.member];
      if (events instanceof EventStream) {
        // Deadlines and cancellation cover the events until they end.
        retained = true; retainUntilSettled(events, cleanup);
      }
      if (operation.output?.streaming && operation.output.payload) {
        const fields = result as Record<string, unknown>;
        const body = fields[operation.output.payload];
        if (body instanceof ByteStream) {
          fields[operation.output.payload] = new ByteStream(lifetimeStream(body.open(), controller.signal, cleanup), { length: body.length });
          retained = true;
        }
      }
      return result;
    } finally { if (!retained) cleanup(); }
  }

  async #send<O>(operation: OperationSpec, input: object, options: RequestOptions): Promise<O> {
    // Preserve write-back of generated tokens, but replay one logical input even
    // when the caller changes its mutable objects while a request is in flight.
    const original = withIdempotencyToken(operation, input) as Record<string, unknown>;
    const streamMember = operation.inputEvents?.member ?? (operation.payload?.streaming ? operation.payload.member : undefined);
    const stream = streamMember ? original[streamMember] : undefined;
    const snapshot = streamMember ? Object.fromEntries(Object.entries(original).filter(([name]) => name !== streamMember)) : original;
    const values = snapshotValue(snapshot) as Record<string, unknown>;
    if (streamMember && stream !== undefined) values[streamMember] = stream;
    const retry = this.#config.retry === false ? { maxAttempts: 1 } : (this.#config.retry ?? {});
    // Events are read from the caller's source once: never replayed.
    const maxAttempts = (stream instanceof ByteStream && !stream.replayable) || operation.inputEvents ? 1 : Math.max(1, retry.maxAttempts ?? 3);
    const streaming = stream instanceof ByteStream || operation.inputEvents !== undefined;
    // Fills defaulted checksum members into the private snapshot only.
    const checksum = planChecksum(operation.checksum, values, this.#config.requestChecksumCalculation ?? "WHEN_SUPPORTED",
      this.#config.responseChecksumValidation ?? "WHEN_SUPPORTED", streaming);
    for (let attempt = 1; ; attempt++) {
      // Invalid inputs are caller errors, not transport failures.
      options.signal?.throwIfAborted();
      const signing = operation.inputEvents ? eventSigning() : undefined;
      const prepared = this.#request(operation, values, options, signing); let request = prepared.request;
      if (streaming && checksum.header !== undefined && !request.headers.has(checksum.header)) {
        // Hashing a stream first would buffer it; the caller supplies the value instead.
        const error = new TypeError(`${operation.name}: a streamed body cannot be checksummed before it is sent; supply the ${checksum.header} header`);
        await request.body?.cancel(error).catch(() => {});
        throw error;
      }
      try { request = await authenticate(request, operation.auth ?? ((operation.bearer ?? this.#bearer) ? [{ kind: "bearer" }] : []), operation.optionalAuth ?? false, this.#config, prepared.properties, streaming, operation.unsignedPayload); }
      catch (error) { await request.body?.cancel(error).catch(() => {}); throw error; }
      let response: Response | undefined;
      let text = "";
      let bytes: Uint8Array | ByteStream | undefined;
      try {
        response = await this.#dispatch(request, signingRequests.get(request), streaming ? undefined : checksum, signing);
        if (options.signal?.aborted) {
          void response.body?.cancel().catch(() => {});
          options.signal.throwIfAborted();
        }
        // Fetch resolves at the headers; required bodies can still fail.
        // A successful operation without a modeled body needs only headers;
        // a payload is read as the bytes sent, never as text.
        if (response.ok && operation.output?.events) {
          // Read below, outside the retry: events are not replayed.
        } else if (response.ok && operation.output?.payload !== undefined && operation.output.streaming) {
          let source = response.body ?? new ReadableStream<Uint8Array>({ start(controller) { controller.close(); } });
          const expected = responseChecksum(checksum, response.headers);
          if (expected !== undefined) source = verifyingStream(source, expected.algorithm, expected.expected, () => this.#config.checksumValidated?.(operation.name, expected.algorithm));
          bytes = new ByteStream(lifetimeStream(source, options.signal, () => {}, undefined, operation.output.minimum, operation.output.maximum));
        } else if (response.ok && operation.output?.payload !== undefined) {
          bytes = await readResponse(response, options.signal);
        } else if (!response.ok || operation.output?.body || checksum.response.length > 0) {
          bytes = await readResponse(response, options.signal);text = new TextDecoder().decode(bytes);
        }
      } catch (error) {
        if (isAbort(error, options.signal) || error instanceof AuthenticationError) throw error;
        const network = new NetworkError(error);
        if (!operation.retrySafe || attempt >= maxAttempts) throw network;
        const delay = response === undefined ? undefined : retryAfter(response);
        await sleep(delay ?? backoff(attempt, retry), options.signal);
        continue;
      }
      if (response.ok && operation.output?.events) return this.#receiveEvents<O>(operation, operation.output.events, response, checksum, options.signal);
      if (response.ok) {
        // A streamed body is verified as it is read. A corrupted response is
        // not retried: the request may have taken effect.
        const expected = responseChecksum(checksum, response.headers);
        if (expected !== undefined && !(bytes instanceof ByteStream)) {
          if (!sameChecksum(expected.expected, await computeChecksum(expected.algorithm, bytes ?? new Uint8Array()))) throw new ChecksumError(expected.algorithm);
          this.#config.checksumValidated?.(operation.name, expected.algorithm);
        }
        try { return this.#decodeOutput(operation, response, text, bytes) as O; }
        catch (error) { if (bytes instanceof ByteStream) await bytes.cancel(error); throw error; }
      }
      const error = this.#decodeError(operation, response, text, bytes instanceof Uint8Array ? bytes : new Uint8Array());
      const throttled = error.$retryable?.throttling === true;
      const transient = error.$retryable !== undefined || RETRYABLE_STATUS.has(response.status);
      if (!(throttled || (operation.retrySafe && transient)) || attempt >= maxAttempts) throw error;
      await sleep(retryAfter(response) ?? backoff(attempt, retry), options.signal);
    }
  }

  async #receiveEvents<O>(operation: OperationSpec, spec: EventStreamSpec, response: Response, checksum: ChecksumPlan, signal: AbortSignal | undefined): Promise<O> {
    const output = operation.output!;
    const settled = new Settlement();
    let source = response.body ?? new ReadableStream<Uint8Array>({ start(controller) { controller.close(); } });
    const expected = responseChecksum(checksum, response.headers);
    if (expected !== undefined) source = verifyingStream(source, expected.algorithm, expected.expected, () => this.#config.checksumValidated?.(operation.name, expected.algorithm));
    const reader = new EventMessageReader(lifetimeStream(source, signal, () => settled.settle()));
    try {
      // Without HTTP bindings the output's other members arrive first, as an initial-response event.
      let document: unknown = {};
      let pending: EventMessage | undefined;
      if (spec.initial) {
        const first = await reader.next();
        if (first !== undefined && decodeEvent(spec, first).kind === "initial") {
          document = readJson(new TextDecoder("utf-8", { fatal: true }).decode(first.payload)) ?? {};
        } else pending = first;
      }
      const fields = object(document);
      readHeaders(response.headers, output.headers, fields, true, output.schema, output.headerEncodings);
      readDynamicHeaders(response, output, fields);
      const metadata: ResponseMetadata = { status: response.status, requestId: response.headers.get("x-request-id") ?? response.headers.get("x-amzn-requestid") ?? response.headers.get("x-amz-request-id") ?? undefined, headers: response.headers };
      const events = receivedEvents(spec, reader, pending, this.#errors, metadata);
      settlements.set(events as EventStream<unknown>, settled);
      Object.defineProperty(fields, wireName(output.schema, spec.member), { value: events, enumerable: true, writable: true, configurable: true });
      return fromWire(output.schema, fields) as O;
    } catch (error) {
      await reader.cancel(error);
      throw error;
    }
  }

  #dispatch(request: Request, signing?: Signing, checksum?: ChecksumPlan, events?: EventSigning): Promise<Response> {
    const send = this.#config.fetch ?? ((request: Request) => fetch(request));
    const signal = request.signal;
    const checked: Next = async pending => {
      signal.throwIfAborted(); pending.signal.throwIfAborted();
      // Over the final bytes, after every middleware and before signing; a
      // checksum the caller already supplied is kept as it is.
      let headers: Headers | undefined;
      if (checksum?.header !== undefined && checksum.algorithm !== undefined && !pending.headers.has(checksum.header)) {
        const bytes = pending.body === null ? new Uint8Array() : new Uint8Array(await pending.clone().arrayBuffer());
        headers = new Headers(pending.headers);
        headers.set(checksum.header, await computeChecksum(checksum.algorithm, bytes));
      }
      if (events) {
        // Each event is signed as it is sent; the request declares so.
        headers ??= new Headers(pending.headers);
        if (signing) headers.set("x-amz-content-sha256", STREAMING_EVENTS);
        else if (headers.get("x-amz-content-sha256") === STREAMING_EVENTS) headers.delete("x-amz-content-sha256");
      }
      const final = new Request(pending, { signal: AbortSignal.any([signal, pending.signal]), ...(headers ? { headers } : {}), ...(events ? { duplex: "half" } : {}) } as RequestInit);
      const signed = signing ? await signSigV4(final, signing.credentials, signing.scope, signing.streaming) : final;
      if (events) {
        const seed = /Signature=([0-9a-f]+)/.exec(signed.headers.get("authorization") ?? "")?.[1];
        events.resolve(signing && seed ? new EventSigner(signing.credentials, signing.scope, seed) : undefined);
      }
      return send(signed);
    };
    const chain = this.#middleware.reduceRight<Next>((next, middleware) => request => {
      signal.throwIfAborted(); request.signal.throwIfAborted(); return middleware(request, next);
    }, checked);
    return chain(request);
  }

  #request(operation: OperationSpec, values: Record<string, unknown>, options: RequestOptions, events?: EventSigning): {request: Request; properties: Readonly<Record<string,unknown>>} {
    const encoded = toWire(operation.input, values) as Record<string, unknown>;
    const path = operation.path
      .map(segment => {
        if (typeof segment === "string") return segment;
        const value = values[segment.label];
        if (value === undefined || value === null || value === "") {
          throw new TypeError(`${operation.name}: \`${segment.label}\` is required`);
        }
        const text = scalar(value, segment.schema);
        // URL and Fetch normalize even percent-encoded dot segments. Never
        // silently change the requested resource when it cannot be preserved.
        const segments = segment.greedy ? text.split("/") : [text];
        if (segments.some(part => part === "." || part === "..")) throw new TypeError(`${operation.name}: dot-segment label \`${segment.label}\` is unsupported`);
        return segments.map(encodeURIComponent).join("/");
      })
      .join("/");
    const endpoint = this.#endpointRules ? resolveEndpoint(this.#endpointRules, operation.endpointParameters ?? {}, values, this.#config.endpointParameters ?? {}, { "AWS::Region": this.#config.region, "SDK::Endpoint": this.#endpoint || undefined, ...this.#config.endpointBuiltins }) : { url: this.#endpoint, headers: {}, properties: {} };
    const base = operation.endpoint && !this.#config.disableHostPrefix ? hostPrefix(endpoint.url, operation.endpoint, values) : endpoint.url;
    const url = new URL(`${base.replace(/\/+$/, "")}/${path}`);
    for (const [key, value] of operation.literalQuery ?? []) url.searchParams.append(key, value ?? "");
    for (const [member, param, schema] of operation.query ?? []) {
      const value = values[member];
      if (value === undefined || value === null) continue;
      const items = Array.isArray(value) ? value : [value];
      const itemSchema = schema?.kind === "list" ? schema.member : schema;
      for (const item of items) url.searchParams.append(param, scalar(item, itemSchema));
    }
    if (operation.queryParams && values[operation.queryParams] != null) {
      const claimed = new Set([...(operation.query ?? []).map(([, name]) => name), ...(operation.literalQuery ?? []).map(([name]) => name)]);
      for (const [key, value] of Object.entries(object(values[operation.queryParams]))) {
        if (claimed.has(key) || value == null) continue;
        for (const item of Array.isArray(value) ? value : [value]) url.searchParams.append(key, scalar(item, undefined));
      }
    }
    const headers = new Headers();
    for (const [name, values] of Object.entries(endpoint.headers)) for (const value of values) headers.append(name, value);
    for (const [name, value] of Object.entries({ ...this.#config.headers, ...options.headers })) headers.set(name, value);
    const explicit = new Set((operation.headers ?? []).map(([, name]) => name.toLowerCase()));
    for (const [member, prefix] of operation.prefixHeaders ?? []) {
      if (values[member] == null) continue;
      for (const [key, value] of Object.entries(object(values[member]))) {
        const name = prefix + key;
        if (!explicit.has(name.toLowerCase()) && value != null) headers.set(name, operation.headerEncodings?.[member]==="base64"?encodeMedia(value):String(value));
      }
    }
    for (const [member, header, schema] of operation.headers ?? []) {
      const value = values[member];
      if (value !== undefined && value !== null) headers.set(header, operation.headerEncodings?.[member]==="base64"?(Array.isArray(value)?value.map(encodeMedia).join(", "):encodeMedia(value)):headerValue(value, schema));
    }
    let body: BodyInit | undefined;
    if (operation.body !== undefined) {
      const picked: Record<string, unknown> = Object.create(null);
      for (const member of operation.body) {
        const name = wireName(operation.input, member);
        if (encoded[name] !== undefined) picked[name] = encoded[name];
      }
      body = stringifyJson(picked);
      if (!headers.has("content-type")) headers.set("content-type", "application/json");
    }
    if (operation.payload !== undefined) {
      // The bytes go as they are: a body digest covers exactly these.
      const value = values[operation.payload.member];
      if (value !== undefined && (value !== null || operation.payload.kind==="document")) {
        if (operation.payload.streaming) {
          if (!(value instanceof ByteStream)) throw new TypeError(`${operation.name}: streaming payload must be a ByteStream`);
          if (operation.payload.requiresLength && value.length === undefined) throw new TypeError(`${operation.name}: streaming payload requires a known length`);
          if (value.sha256) headers.set("x-amz-content-sha256", value.sha256);
          if (this.#middleware.includes(contentSha256) && !headers.has("x-amz-content-sha256")) throw new TypeError("streaming contentSha256 requires a precomputed ByteStream sha256");
          if (value.length !== undefined) headers.set("content-length", String(value.length));
          body = lifetimeStream(value.open(), options.signal, () => {}, value.length, operation.payload.minimum, operation.payload.maximum);
        } else if (operation.payload.kind==="string") {
          if (typeof value!=="string") throw new TypeError(`${operation.name}: payload must be a string`);body=value;
        } else if (operation.payload.kind==="document") {
          body=stringifyJson(encoded[wireName(operation.input,operation.payload.member)]);
        } else {
          if (!(value instanceof Uint8Array)) throw new TypeError(`${operation.name}: payload must be a Uint8Array`);
          body = value as BodyInit;
        }
        if (!headers.has("content-type")) headers.set("content-type", operation.payload.mediaType);
      }
    }
    if (operation.protocol==="awsJson1_0" || operation.protocol==="awsJson1_1") {
      body ??="{}";headers.set("content-type",`application/x-amz-json-${operation.protocol==="awsJson1_0"?"1.0":"1.1"}`);headers.set("x-amz-target",operation.target!);
    }
    if (operation.inputEvents && events) {
      const source = values[operation.inputEvents.member] ?? [];
      if (typeof source !== "object" || (!(Symbol.asyncIterator in source) && !(Symbol.iterator in source))) throw new TypeError(`${operation.name}: an event stream member is an EventStream or an iterable of events`);
      const sigV4 = operation.auth?.some(scheme => scheme.kind === "sigV4") ?? false;
      if (this.#middleware.includes(contentSha256) && !sigV4) throw new TypeError("an event stream has no body digest to precompute");
      // Without HTTP bindings the document the body would have been is the stream's first message.
      const initial = operation.inputEvents.initial ? (typeof body === "string" ? body : "{}") : undefined;
      body = eventBody(operation.inputEvents, source as AsyncIterable<unknown> | Iterable<unknown>, initial, events);
      headers.set("content-type", EVENT_STREAM_CONTENT_TYPE);
      // Declared before middleware runs, so a digest middleware leaves the stream alone; removed again if no signature follows.
      if (sigV4) headers.set("x-amz-content-sha256", STREAMING_EVENTS);
    }
    const init: RequestInit & { duplex?: "half" } = { method: operation.method, headers, body, signal: options.signal };
    if (body instanceof ReadableStream) init.duplex = "half";
    return {request: new Request(url, init),properties:endpoint.properties};
  }

  #decodeOutput(operation: OperationSpec, response: Response, text: string, bytes: Uint8Array | ByteStream | undefined): unknown {
    const output = operation.output;
    if (output === undefined) return {};
    const body = output.payload !== undefined
      ? { [wireName(output.schema, output.payload)]: decodePayload(output.payloadKind,bytes ?? new Uint8Array()) }
      : output.body ? readJson(text) : {};
    const fields = object(body ?? {});
    readHeaders(response.headers, output.headers, fields, true, output.schema,output.headerEncodings);
    readDynamicHeaders(response, output, fields);
    return fromWire(output.schema, fields);
  }

  #decodeError(operation: OperationSpec, response: Response, text: string, bytes: Uint8Array): ServiceError {
    let parsed: unknown;
    try { parsed = readJson(text); } catch { /* An HTTP error may have a non-JSON body. */ }
    const body = (parsed !== null && typeof parsed === "object" ? parsed : {}) as Record<string, unknown>;
    const metadata: ResponseMetadata = {
      status: response.status,
      requestId: response.headers.get("x-request-id") ?? response.headers.get("x-amzn-requestid") ?? response.headers.get("x-amz-request-id") ?? stringField(body, "requestId"),
      headers: response.headers,
    };
    const aws=operation.protocol!==undefined && operation.protocol!=="crucibleJson";
    const discriminator=response.headers.get("x-crucible-error") ?? (aws?response.headers.get("x-amzn-errortype") ?? stringField(body,"__type") ?? stringField(body,"code"):null) ?? errorName(body,operation,this.#errors,response.status);
    const name=aws?discriminator?.split(":")[0].split("#").at(-1):discriminator;
    const spec = name !== undefined && operation.errors.includes(name) && Object.hasOwn(this.#errors, name)
      ? this.#errors[name] : undefined;
    if (spec === undefined) {
      const message = stringField(body, "message") ?? (aws?stringField(body,"Message"):undefined) ?? `${operation.name} failed with HTTP ${response.status}`;
      return new ServiceException(name ?? "UnknownError", message, metadata, response.status >= 500 ? "server" : "client");
    }
    let fields: Record<string, unknown>;
    try {
      if (spec.payload!==undefined) Object.defineProperty(body,wireName(spec.schema,spec.payload),{value:decodePayload(spec.payloadKind,bytes),writable:true,configurable:true,enumerable:true});
      if (aws && body.message===undefined && typeof body.Message==="string") body.message=body.Message;
      readHeaders(response.headers, spec.headers, body, true, spec.schema,spec.headerEncodings);
      readDynamicHeaders(response, spec, body);
      fields = fromWire(spec.schema, body) as Record<string, unknown>;
    } catch (error) {
      if (!(error instanceof TypeError || error instanceof RangeError || error instanceof DOMException)) throw error;
      return new ServiceException(name!, stringField(body, "message") ?? `${operation.name} failed with HTTP ${response.status}`,
        metadata, spec.fault ?? (response.status >= 500 ? "server" : "client"), spec.retryable);
    }
    return spec.create(fields, metadata);
  }
}

/**
 * Which modelled error a response is: the body's `__type` names it. When it
 * does not, the status decides, but only if one error of the operation has it.
 */
function errorName(
  body: Record<string, unknown>,
  operation: OperationSpec,
  errors: Readonly<Record<string, ErrorSpec>>,
  status: number,
): string | undefined {
  const named = stringField(body, "__type");
  if (named !== undefined) return named;
  const byStatus = operation.errors.filter(name => errors[name]?.status === status);
  return byStatus.length === 1 ? byStatus[0] : undefined;
}

function withIdempotencyToken(operation: OperationSpec, input: object): object {
  const member = operation.idempotencyToken;
  const record = input as Record<string, unknown>;
  if (member === undefined || (Object.hasOwn(record, member) && record[member] !== undefined)) return input;
  // Written into the caller's own object when it can be, so resending that
  // object after a failure replays the request instead of repeating it.
  const token = globalThis.crypto.randomUUID();
  if (Object.isExtensible(input)) {
    Object.defineProperty(record, member, { value: token, enumerable: true, writable: true, configurable: true });
    return input;
  }
  return { ...record, [member]: token };
}

function scalar(value: unknown, schema: Schema | undefined): string {
  if (value instanceof Date) {
    const format = schema?.kind === "timestamp" ? schema.format : "date-time";
    return String(formatTimestamp(value, format));
  }
  const encoded = toWire(schema, value);
  return encoded instanceof JsonNumber ? encoded.text : String(encoded);
}

function headerValue(value: unknown, schema: Schema | undefined): string {
  if (!Array.isArray(value)) return scalar(value, schema);
  const member = schema?.kind === "list" ? schema.member : undefined;
  if (member?.kind==="timestamp" && member.format==="http-date") return value.map(item=>scalar(item,member)).join(", ");
  return value
    .map(item => {
      const text = scalar(item, member);
      return text.length === 0 || /[",\\]/.test(text) || text.trim() !== text
        ? `"${text.replace(/["\\]/g, "\\$&")}"` : text;
    })
    .join(", ");
}

function readHeaders(
  headers: Headers,
  bindings: readonly (readonly [string, string, Schema?])[] | undefined,
  into: Record<string, unknown>,
  wire = false,
  shape?: Schema,
  encodings?: Readonly<Record<string,"base64">>,
): void {
  for (const [member, header, schema] of bindings ?? []) {
    const text = headers.get(header);
    if (text !== null) Object.defineProperty(into, wire ? wireName(shape, member) : member, { value: encodings?.[member]==="base64"?(schema?.kind==="list"?splitHeader(text).map(decodeMedia):decodeMedia(text)):parseHeader(text, schema, wire), enumerable: true, writable: true, configurable: true });
  }
}

function readDynamicHeaders(response: Response, spec: { readonly schema?: Schema; readonly headerEncodings?: Readonly<Record<string,"base64">>; readonly responseCode?: string; readonly prefixHeaders?: readonly (readonly [string, string])[] }, into: Record<string, unknown>): void {
  for (const [member, prefix] of spec.prefixHeaders ?? []) {
    const entries: [string, string][] = [];
    response.headers.forEach((value, name) => {
      if (name.toLowerCase().startsWith(prefix.toLowerCase())) entries.push([name.slice(prefix.length), spec.headerEncodings?.[member]==="base64"?decodeMedia(value):value]);
    });
    Object.defineProperty(into, wireName(spec.schema, member), { value: Object.fromEntries(entries), enumerable: true, writable: true, configurable: true });
  }
  if (spec.responseCode) Object.defineProperty(into, wireName(spec.schema, spec.responseCode), { value: response.status, enumerable: true, writable: true, configurable: true });
}

function parseHeader(text: string, schema: Schema | undefined, wire = false): unknown {
  switch (schema?.kind) {
    case "integer": case "bigInteger": case "bigDecimal":
      return wire ? new JsonNumber(text.trim()) : fromWire(schema, new JsonNumber(text.trim()));
    case "number": {
      const trimmed = text.trim();
      const value = schema.nonFinite && ["NaN", "Infinity", "-Infinity"].includes(trimmed) ? trimmed : new JsonNumber(trimmed);
      // The enclosing output schema must see the protocol string, so it converts non-finite values only once.
      return wire ? value : fromWire(schema, value);
    }
    case "boolean":
      return text.trim() === "true";
    case "timestamp":
      return wire ? (schema.format === "epoch-seconds" ? new JsonNumber(text.trim()) : text)
        : parseTimestamp(schema.format === "epoch-seconds" ? Number(text) : text);
    case "list":
      return (schema.member.kind==="timestamp" && schema.member.format==="http-date" && !text.trimStart().startsWith('"') ? text.split(/,\s*(?=(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun),)/) : splitHeader(text)).map(item => parseHeader(item, schema.member, wire));
    default:
      return text;
  }
}

function splitHeader(text: string): string[] {
  if (text.trim().length === 0) return [];
  const items: string[] = [];
  const pattern = /\s*(?:"((?:[^"\\]|\\.)*)"|([^,]*?))\s*(?:,|$)/g;
  for (const match of text.matchAll(pattern)) {
    if (match.index === text.length) break;
    items.push(match[1] !== undefined ? match[1].replace(/\\(.)/g, "$1") : match[2]);
  }
  return items;
}

function readJson(text: string): unknown {
  return text.length === 0 ? undefined : parseJson(text);
}

function encodeMedia(value: unknown): string {
  if (typeof value!=="string") throw new TypeError("media-type header value must be a string");
  return toBase64(new TextEncoder().encode(value));
}
function decodeMedia(value: string): string {return new TextDecoder("utf-8",{fatal:true}).decode(fromBase64(value));}
function decodePayload(kind: "blob" | "string" | "document" | undefined, bytes: Uint8Array | ByteStream): unknown {
  if (kind===undefined || kind==="blob") return bytes;
  if (!(bytes instanceof Uint8Array)) throw new TypeError("non-blob payload cannot be a byte stream");
  const text=new TextDecoder("utf-8",{fatal:true}).decode(bytes);
  return kind==="string"?text:readJson(text);
}

function stringField(body: Record<string, unknown>, name: string): string | undefined {
  const value = body[name];
  return typeof value === "string" ? value : undefined;
}

async function identity<T>(provider: IdentityProvider<T> | undefined): Promise<T | undefined> {
  return typeof provider === "function" ? (provider as () => T | undefined | Promise<T | undefined>)() : provider;
}

async function authenticate(request: Request, schemes: readonly AuthScheme[], optional: boolean, config: ClientConfig, properties: Readonly<Record<string,unknown>>, streaming: boolean, unsignedPayload?: boolean): Promise<Request> {
  if (!schemes.length) return request;
  const authorization = request.headers.get("authorization") ?? "";
  // Explicit credentials for any modeled option take precedence over providers.
  for (const scheme of schemes) {
    if (scheme.kind === "bearer" && /^Bearer\s+\S/i.test(authorization)) return request;
    if (scheme.kind === "basic" && /^Basic\s+\S/i.test(authorization)) return request;
    if (scheme.kind === "sigV4" && authorization.startsWith("AWS4-HMAC-SHA256 ")) return request;
    if (scheme.kind === "apiKey" && (scheme.location === "header" ? request.headers.has(scheme.name) : new URL(request.url).searchParams.has(scheme.name))) return request;
  }
  for (const scheme of schemes) {
    let header: string | undefined;
    if (scheme.kind === "bearer") {
      const token = await identity(config.token);
      if (token) header = `Bearer ${token}`;
    } else if (scheme.kind === "basic") {
      const credentials = await identity(config.basicCredentials);
      if (credentials) {
        if (credentials.username.includes(":")) throw new AuthenticationError("basic username cannot contain a colon");
        header = `Basic ${toBase64(new TextEncoder().encode(`${credentials.username}:${credentials.password}`))}`;
      }
    } else if (scheme.kind === "sigV4") {
      const credentials = await identity(config.awsCredentials);
      if (credentials) {
        request.signal.throwIfAborted();
        const overrides = (Array.isArray(properties.authSchemes) ? properties.authSchemes : []).find(value=>value && typeof value==="object" && ["sigv4","aws.auth#sigv4"].includes(value.name)) as Record<string,unknown> | undefined;
        signingRequests.set(request,{credentials,scope:{region:typeof overrides?.signingRegion==="string"?overrides.signingRegion:config.region ?? "",name:typeof overrides?.signingName==="string"?overrides.signingName:config.signingName ?? scheme.signingName,disableDoubleEncoding:typeof overrides?.disableDoubleEncoding==="boolean"?overrides.disableDoubleEncoding:undefined,disableNormalizePath:typeof overrides?.disableNormalizePath==="boolean"?overrides.disableNormalizePath:undefined,unsignedPayload},streaming});
        return request;
      }
    } else {
      const key = await identity(config.apiKey);
      if (key !== undefined) {
        request.signal.throwIfAborted();
        if (scheme.location === "query") {
          const url = new URL(request.url); url.searchParams.set(scheme.name, key);
          return new Request(url, request);
        }
        request.headers.set(scheme.name, scheme.scheme ? `${scheme.scheme} ${key}` : key);
        return request;
      }
    }
    request.signal.throwIfAborted();
    if (header !== undefined) { request.headers.set("authorization", header); return request; }
  }
  // Preserve the established bearer-only contract: the service can decide
  // whether an anonymous request is permitted. Other required schemes fail early.
  if (optional || schemes.every(scheme => scheme.kind === "bearer")) return request;
  throw new AuthenticationError("no identity is available for the operation authentication schemes");
}

/**
 * Adds `x-amz-content-sha256` to a request with a body, and to every POST,
 * PUT and PATCH even without one (the empty body's digest): a front door
 * that signs requests to its origin needs the digest from the caller, and
 * may refuse a write method that arrives without it.
 */
export const contentSha256: Middleware = async (request, next) => {
  if (request.headers.has("x-amz-content-sha256")) return next(request);
  const writes = request.method === "POST" || request.method === "PUT" || request.method === "PATCH";
  if (request.body === null && !writes) return next(request);
  const bytes = request.body === null ? new ArrayBuffer(0) : await request.clone().arrayBuffer();
  const digest = new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", bytes));
  request.headers.set("x-amz-content-sha256", Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join(""));
  return next(request);
};

/** Runs a call's cleanup once its received events have ended, however they end. */
class Settlement {
  #settled = false;
  #cleanup?: () => void;
  settle(): void { if (this.#settled) return; this.#settled = true; this.#cleanup?.(); }
  retain(cleanup: () => void): void { if (this.#settled) cleanup(); else this.#cleanup = cleanup; }
}
const settlements = new WeakMap<EventStream<unknown>, Settlement>();
function retainUntilSettled(events: EventStream<unknown>, cleanup: () => void): void {
  const settlement = settlements.get(events);
  if (settlement) settlement.retain(cleanup); else cleanup();
}

/** Keeps transport ownership until EOF or cancellation without eager buffering. */
function lifetimeStream(source: ReadableStream<Uint8Array>, signal: AbortSignal | undefined, cleanup: () => void, length?: number, minimum?: number, maximum?: number): ReadableStream<Uint8Array> {
  const reader = source.getReader();
  let finished = false;
  let size = 0;
  let controller: ReadableStreamDefaultController<Uint8Array>;
  const finish = () => {
    if (finished) return;
    finished = true;
    signal?.removeEventListener("abort", abort);
    reader.releaseLock(); cleanup();
  };
  const abort = () => {
    if (finished) return;
    controller.error(signal?.reason);
    void reader.cancel(signal?.reason).catch(() => {}).finally(finish);
  };
  return new ReadableStream<Uint8Array>({
    start(value) { controller = value; signal?.addEventListener("abort", abort, { once: true }); if (signal?.aborted) abort(); },
    async pull() {
      try {
        signal?.throwIfAborted();
        const next = await reader.read();
        if (finished) return;
        signal?.throwIfAborted();
        if (next.done) {
          if ((length !== undefined && size !== length) || (minimum !== undefined && size < minimum)) throw new TypeError("stream ended before its declared length");
          controller.close(); finish(); return;
        }
        if (!(next.value instanceof Uint8Array)) throw new TypeError("stream chunks must be Uint8Array");
        size += next.value.byteLength;
        if (!Number.isSafeInteger(size) || (length !== undefined && size > length) || (maximum !== undefined && size > maximum)) throw new TypeError("stream exceeds its declared length");
        controller.enqueue(next.value);
      } catch (error) {
        if (!finished) { controller.error(error); await reader.cancel(error).catch(() => {}); finish(); }
      }
    },
    async cancel(reason) { try { await reader.cancel(reason); } finally { finish(); } },
  }, { highWaterMark: 0 });
}

async function readResponse(response: Response, signal: AbortSignal | undefined): Promise<Uint8Array> {
  if (!response.body) return new Uint8Array();
  const reader = response.body.getReader();
  const abort = () => { void reader.cancel(signal?.reason).catch(() => {}); };
  signal?.addEventListener("abort", abort, { once: true });
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    for (;;) {
      signal?.throwIfAborted();
      const next = await reader.read();
      signal?.throwIfAborted();
      if (next.done) break;
      chunks.push(next.value); length += next.value.length;
    }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
    return bytes;
  } finally {
    signal?.removeEventListener("abort", abort);
    reader.releaseLock();
  }
}

export function abortable<T>(work: Promise<T>, signal: AbortSignal): Promise<T> {
  return new Promise((resolve, reject) => {
    const abort = () => reject(signal.reason);
    signal.addEventListener("abort", abort, { once: true });
    if (signal.aborted) abort();
    work.then(resolve, reject).finally(() => signal.removeEventListener("abort", abort));
  });
}

function isAbort(error: unknown, signal: AbortSignal | undefined): boolean {
  return signal?.aborted === true || (error instanceof Error && error.name === "AbortError");
}

/** Full jitter: a uniform delay up to an exponentially growing cap. */
function backoff(attempt: number, retry: RetryConfig): number {
  const base = retry.baseDelayMs ?? 100;
  const max = retry.maxDelayMs ?? 20_000;
  return Math.random() * Math.min(max, base * 2 ** (attempt - 1));
}

function retryAfter(response: Response): number | undefined {
  const header = response.headers.get("retry-after")?.trim();
  if (!header) return undefined;
  const seconds = Number(header);
  if (Number.isFinite(seconds)) return seconds >= 0 ? Math.min(Number.MAX_SAFE_INTEGER, seconds * 1000) : undefined;
  // Numeric overflows are still delays, not dates or a request to retry now.
  if (/^\+?\d+(?:\.\d+)?(?:e\+?\d+)?$/i.test(header)) return Number.MAX_SAFE_INTEGER;
  const date = Date.parse(header);
  return Number.isFinite(date) ? Math.max(0, date - Date.now()) : undefined;
}

export async function sleep(ms: number, signal: AbortSignal | undefined): Promise<void> {
  // Browsers and Node clamp overflowing timer delays to approximately 1 ms.
  // Chunk long server delays while keeping every chunk abortable.
  const maximum = 2_147_483_647;
  do {
    const delay = Math.min(maximum, ms);
    await sleepChunk(delay, signal);
    ms -= delay;
  } while (ms > 0);
}

function sleepChunk(ms: number, signal: AbortSignal | undefined): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(signal.reason);
    const timer = setTimeout(done, ms);
    function done() {
      signal?.removeEventListener("abort", abort);
      resolve();
    }
    function abort() {
      clearTimeout(timer);
      reject(signal!.reason);
    }
    signal?.addEventListener("abort", abort, { once: true });
  });
}

/**
 * Pages of a paginated operation, following its output token until a page
 * comes back without one. A service that returns the token it was sent would
 * loop forever, so that is an error.
 */
export async function* paginate<I extends object, O>(
  call: (input: I) => Promise<O>,
  input: I,
  inputToken: string,
  outputToken: readonly string[],
): AsyncGenerator<O, void, undefined> {
  let token = (input as Record<string, unknown>)[inputToken];
  for (;;) {
    const page = await call({ ...input, [inputToken]: token });
    let next: unknown = page;
    for (const step of outputToken) next = (next as Record<string, unknown> | undefined)?.[step];
    const last = next === undefined || next === null || next === "";
    if (!last && next === token) throw new Error("pagination stalled: the service returned the token it was sent");
    yield page;
    if (last) return;
    token = next;
  }
}

/** Numeric text for the companion query interpreter, without Number coercion. */
export function numericText(value: unknown): string | undefined {
  if (value instanceof Decimal || value instanceof JsonNumber) return value instanceof Decimal ? value.toString() : value.text;
  if (typeof value === "bigint" || (typeof value === "number" && Number.isFinite(value))) return String(value);
  return undefined;
}

/** Compiled AST literals retain exact numeric tokens; structural indices stay numbers. */
export function parseBehavior(text: string): unknown {
  const convert = (value: unknown): unknown => {
    if (value instanceof JsonNumber) return Number(value.text);
    if (Array.isArray(value)) return value.map(convert);
    if (value !== null && typeof value === "object") {
      const record = value as Record<string, unknown>;
      return Object.fromEntries(Object.entries(record).map(([key, value]) => [key, record.kind === "literal" && key === "value" ? value : convert(value)]));
    }
    return value;
  };
  return convert(parseJson(text));
}
