// crucible TypeScript runtime. Generated clients carry a copy of this file
// and import nothing else, so a client and its runtime can never disagree.
// Edit it in the crucible repository (crates/crucible-emit-ts/runtime/), never
// in a generated package.

/** Any JSON value: what a Smithy `document` holds. */
export type Document = null | boolean | number | string | Document[] | { [key: string]: Document };

/** A union member or result that carries no value. On the wire it is `{}`. */
export type Unit = Record<string, never>;

export type TimestampFormat = "date-time" | "http-date" | "epoch-seconds";

/** Static wire conversion and validation metadata emitted from the Smithy IR. */
export type Schema =
  | { readonly kind: "string" | "number" | "boolean" | "document" | "blob" }
  | { readonly kind: "integer"; readonly bits: 8 | 16 | 32 | 64 }
  | { readonly kind: "timestamp"; readonly format: TimestampFormat }
  | { readonly kind: "list"; readonly member: Schema }
  | { readonly kind: "map"; readonly value: Schema }
  | {
      readonly kind: "structure" | "union";
      readonly members: Readonly<Record<string, Schema>>;
      readonly defaults?: Readonly<Record<string, unknown>>;
      /** Nonoptional client members, settled by the IR (not inferred here). */
      readonly required?: readonly string[];
    };

export const string: Schema = { kind: "string" };
export const number: Schema = { kind: "number" };
export const boolean: Schema = { kind: "boolean" };
export const document: Schema = { kind: "document" };
export const blob: Schema = { kind: "blob" };
export const integer = (bits: 8 | 16 | 32 | 64): Schema => ({ kind: "integer", bits });
export const timestamp = (format: TimestampFormat): Schema => ({ kind: "timestamp", format });
export function list(member: Schema | (() => Schema)): Schema {
  return { kind: "list", get member() { return typeof member === "function" ? member() : member; } };
}
export function map(value: Schema | (() => Schema)): Schema {
  return { kind: "map", get value() { return typeof value === "function" ? value() : value; } };
}

/** Thunks also support recursive list/map aliases without declaration-order work. */
export function structure(members: () => Record<string, Schema>, defaults?: Record<string, unknown>, required?: readonly string[]): Schema {
  let resolved: Record<string, Schema> | undefined;
  return { kind: "structure", get members() { return (resolved ??= members()); }, defaults, required };
}
export function union(members: () => Record<string, Schema>): Schema {
  let resolved: Record<string, Schema> | undefined;
  return { kind: "union", get members() { return (resolved ??= members()); } };
}

/** An internal numeric token. It never escapes decoding into a public model. */
class JsonNumber {
  constructor(readonly text: string) {}
}

function exactInteger(value: unknown, bits: 8 | 16 | 32 | 64, encoding: boolean): bigint {
  if (encoding && (bits === 64 ? typeof value !== "bigint" : typeof value !== "number")) {
    throw new TypeError(bits === 64 ? "a long is a bigint" : "an integer is a number");
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
    if (digits.length + shift > 19) throw new TypeError("integer outside range");
    digits += "0".repeat(shift);
  }
  if (digits.length > 19) throw new TypeError("integer outside range");
  const result = BigInt(match[1] + digits);
  const maximum = 1n << BigInt(bits - 1);
  if (result < -maximum || result >= maximum) throw new TypeError(`integer outside signed ${bits}-bit range`);
  return result;
}

/** Restore the documented JavaScript number representation of JSON documents. */
function jsonValue(value: unknown): unknown {
  if (value instanceof JsonNumber) return jsonValue(Number(value.text));
  if (typeof value === "number" && !Number.isFinite(value)) throw new TypeError("non-finite JSON number");
  if (value instanceof Uint8Array || value instanceof Date) return value;
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
    case "integer": {
      const result = exactInteger(value, schema.bits, true);
      return schema.bits === 64 ? result : Number(result);
    }
    case "string": case "boolean": case "number":
      if (typeof value !== schema.kind || (typeof value === "number" && !Number.isFinite(value))) throw new TypeError(`expected ${schema.kind}`);
      return value;
    case "timestamp": return formatTimestamp(value as Date, schema.format);
    case "blob": return toBase64(value as Uint8Array);
    case "list":
      if (!Array.isArray(value)) throw new TypeError("expected array");
      return value.map(item => toWire(schema.member, item));
    case "map": return mapValues(object(value), item => toWire(schema.value, item));
    case "structure": case "union": return convertMembers(schema, object(value), toWire);
    default: return value;
  }
}

export function fromWire(schema: Schema | undefined, value: unknown): unknown {
  if (schema === undefined || schema.kind === "document" || value === undefined || value === null) return jsonValue(value);
  switch (schema.kind) {
    case "integer": {
      const result = exactInteger(value, schema.bits, false);
      return schema.bits === 64 ? result : Number(result);
    }
    case "number": {
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
      return value.map(item => fromWire(schema.member, item));
    case "map": return mapValues(object(value), item => fromWire(schema.value, item));
    case "structure": case "union": return convertMembers(schema, object(value), fromWire);
  }
}

function convertMembers(
  schema: Extract<Schema, { kind: "structure" | "union" }>,
  value: Record<string, unknown>,
  convert: (schema: Schema | undefined, value: unknown) => unknown,
): Record<string, unknown> {
  const out = { ...value };
  if (schema.kind === "union" && Object.keys(out).filter(name => out[name] !== undefined).length !== 1) {
    throw new TypeError("a union must have exactly one member");
  }
  if (convert === fromWire) {
    for (const [name, fallback] of Object.entries(schema.defaults ?? {})) {
      if (!Object.hasOwn(out, name) || out[name] === undefined) Object.defineProperty(out, name, { value: fallback, enumerable: true, writable: true, configurable: true });
    }
  }
  for (const name of schema.required ?? []) {
    if (!Object.hasOwn(out, name) || out[name] === undefined) throw new TypeError(`missing required member: ${name}`);
    if (out[name] === null && schema.members[name]?.kind !== "document") throw new TypeError(`null required member: ${name}`);
  }
  for (const name of Object.keys(out)) {
    const member = Object.hasOwn(schema.members, name) ? schema.members[name] : undefined;
    out[name] = convert(member, out[name]);
  }
  return out;
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

export function parseTimestamp(value: string | number): Date {
  const date = typeof value === "number" ? new Date(value * 1000) : new Date(value);
  if (Number.isNaN(date.getTime())) throw new TypeError(`not a timestamp: ${String(value)}`);
  return date;
}

/** Where each member of an operation's input and output travels. */
export interface OperationSpec {
  readonly name: string;
  readonly method: string;
  /** Literal path segments and `{ label: member }` placeholders. */
  readonly path: readonly (string | { readonly label: string; readonly schema?: Schema })[];
  readonly literalQuery?: readonly (readonly [string, string | undefined])[];
  readonly query?: readonly (readonly [member: string, param: string, schema?: Schema])[];
  readonly headers?: readonly (readonly [member: string, header: string, schema?: Schema])[];
  /** Input members in the JSON body; absent when the request has no body. */
  readonly body?: readonly string[];
  /** The input member that is the whole body, raw (`@httpPayload`), and its content type. */
  readonly payload?: { readonly member: string; readonly mediaType: string };
  readonly input?: Schema;
  readonly output?: {
    readonly body: boolean;
    /** The output member that is the whole body, read as bytes. */
    readonly payload?: string;
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

/** The request never got a response: DNS, connection, TLS or CORS. */
export class NetworkError extends Error {
  constructor(readonly cause: unknown) {
    super(cause instanceof Error ? cause.message : String(cause));
    this.name = "NetworkError";
  }
}

export interface ErrorSpec {
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
export type TokenProvider = string | (() => string | undefined | Promise<string | undefined>);

export interface RetryConfig {
  /** Attempts in all, the first included. */
  readonly maxAttempts?: number;
  readonly baseDelayMs?: number;
  readonly maxDelayMs?: number;
}

export interface ClientConfig {
  readonly endpoint?: string;
  /** Sent as a bearer token. A function is called for every attempt, so a refreshed token is picked up. */
  readonly token?: TokenProvider;
  readonly fetch?: Fetch;
  /** Headers sent on every request. */
  readonly headers?: Readonly<Record<string, string>>;
  /** Runs after the client's own middleware, nearest the network last. */
  readonly middleware?: readonly Middleware[];
  readonly retry?: RetryConfig | false;
}

export interface RequestOptions {
  readonly signal?: AbortSignal;
  readonly headers?: Readonly<Record<string, string>>;
}

const RETRYABLE_STATUS = new Set([500, 502, 503, 504]);

export class Transport {
  readonly #config: ClientConfig;
  readonly #endpoint: string;
  readonly #middleware: readonly Middleware[];
  readonly #errors: Readonly<Record<string, ErrorSpec>>;

  constructor(
    config: ClientConfig,
    defaults: { readonly endpoint?: string; readonly middleware: readonly Middleware[]; readonly bearer: boolean },
    errors: Readonly<Record<string, ErrorSpec>>,
  ) {
    const endpoint = config.endpoint ?? defaults.endpoint;
    if (!endpoint) throw new TypeError("no endpoint: pass `endpoint` in the client config");
    this.#config = config;
    this.#endpoint = endpoint.replace(/\/+$/, "");
    this.#errors = errors;
    const auth = defaults.bearer && config.token !== undefined ? [bearer(config.token)] : [];
    this.#middleware = [...auth, ...defaults.middleware, ...(config.middleware ?? [])];
  }

  async send<O>(operation: OperationSpec, input: object, options: RequestOptions = {}): Promise<O> {
    const values = withIdempotencyToken(operation, input) as Record<string, unknown>;
    const retry = this.#config.retry === false ? { maxAttempts: 1 } : (this.#config.retry ?? {});
    const maxAttempts = Math.max(1, retry.maxAttempts ?? 3);
    for (let attempt = 1; ; attempt++) {
      // Invalid inputs are caller errors, not transport failures.
      const request = this.#request(operation, values, options);
      let response: Response | undefined;
      let text = "";
      let bytes: Uint8Array | undefined;
      try {
        response = await this.#dispatch(request);
        // Fetch resolves at the headers; required bodies can still fail.
        // A successful operation without a modeled body needs only headers;
        // a payload is read as the bytes sent, never as text.
        if (response.ok && operation.output?.payload !== undefined) {
          bytes = new Uint8Array(await response.arrayBuffer());
        } else if (!response.ok || operation.output?.body) {
          text = await response.text();
        }
      } catch (error) {
        if (isAbort(error, options.signal)) throw error;
        const network = new NetworkError(error);
        if (!operation.retrySafe || attempt >= maxAttempts) throw network;
        const delay = response === undefined ? undefined : retryAfter(response);
        await sleep(delay ?? backoff(attempt, retry), options.signal);
        continue;
      }
      if (response.ok) return this.#decodeOutput(operation, response, text, bytes) as O;
      const error = this.#decodeError(operation, response, text);
      const throttled = error.$retryable?.throttling === true;
      const transient = error.$retryable !== undefined || RETRYABLE_STATUS.has(response.status);
      if (!(throttled || (operation.retrySafe && transient)) || attempt >= maxAttempts) throw error;
      await sleep(retryAfter(response) ?? backoff(attempt, retry), options.signal);
    }
  }

  #dispatch(request: Request): Promise<Response> {
    const send = this.#config.fetch ?? ((request: Request) => fetch(request));
    const chain = this.#middleware.reduceRight<Next>((next, middleware) => request => middleware(request, next), send);
    return chain(request);
  }

  #request(operation: OperationSpec, values: Record<string, unknown>, options: RequestOptions): Request {
    const encoded = toWire(operation.input, values) as Record<string, unknown>;
    const path = operation.path
      .map(segment => {
        if (typeof segment === "string") return segment;
        const value = values[segment.label];
        if (value === undefined || value === null || value === "") {
          throw new TypeError(`${operation.name}: \`${segment.label}\` is required`);
        }
        return encodeURIComponent(scalar(value, segment.schema));
      })
      .join("/");
    const url = new URL(`${this.#endpoint}/${path}`);
    for (const [key, value] of operation.literalQuery ?? []) url.searchParams.append(key, value ?? "");
    for (const [member, param, schema] of operation.query ?? []) {
      const value = values[member];
      if (value === undefined || value === null) continue;
      const items = Array.isArray(value) ? value : [value];
      const itemSchema = schema?.kind === "list" ? schema.member : schema;
      for (const item of items) url.searchParams.append(param, scalar(item, itemSchema));
    }
    const headers = new Headers({ ...this.#config.headers, ...options.headers });
    for (const [member, header, schema] of operation.headers ?? []) {
      const value = values[member];
      if (value !== undefined && value !== null) headers.set(header, headerValue(value, schema));
    }
    let body: BodyInit | undefined;
    if (operation.body !== undefined) {
      const picked: Record<string, unknown> = Object.create(null);
      for (const member of operation.body) if (encoded[member] !== undefined) picked[member] = encoded[member];
      body = stringifyJson(picked);
      headers.set("content-type", "application/json");
    }
    if (operation.payload !== undefined) {
      // The bytes go as they are: a body digest covers exactly these.
      const value = values[operation.payload.member];
      if (value !== undefined && value !== null) {
        if (!(value instanceof Uint8Array)) {
          throw new TypeError(`${operation.name}: \`${operation.payload.member}\` is a Uint8Array`);
        }
        body = value as BodyInit;
        headers.set("content-type", operation.payload.mediaType);
      }
    }
    return new Request(url, { method: operation.method, headers, body, signal: options.signal });
  }

  #decodeOutput(operation: OperationSpec, response: Response, text: string, bytes: Uint8Array | undefined): unknown {
    const output = operation.output;
    if (output === undefined) return {};
    const body = output.payload !== undefined
      ? { [output.payload]: bytes ?? new Uint8Array() }
      : output.body ? readJson(text) : {};
    const fields = object(body ?? {});
    readHeaders(response.headers, output.headers, fields, true);
    return fromWire(output.schema, fields);
  }

  #decodeError(operation: OperationSpec, response: Response, text: string): ServiceError {
    let parsed: unknown;
    try { parsed = readJson(text); } catch { /* An HTTP error may have a non-JSON body. */ }
    const body = (parsed !== null && typeof parsed === "object" ? parsed : {}) as Record<string, unknown>;
    const metadata: ResponseMetadata = {
      status: response.status,
      requestId: response.headers.get("x-request-id") ?? stringField(body, "requestId"),
      headers: response.headers,
    };
    const name = errorName(body, operation, this.#errors, response.status);
    const spec = name !== undefined && operation.errors.includes(name) && Object.hasOwn(this.#errors, name)
      ? this.#errors[name] : undefined;
    if (spec === undefined) {
      const message = stringField(body, "message") ?? `${operation.name} failed with HTTP ${response.status}`;
      return new ServiceError(name ?? "UnknownError", message, metadata, response.status >= 500 ? "server" : "client");
    }
    let fields: Record<string, unknown>;
    try {
      readHeaders(response.headers, spec.headers, body, true);
      fields = fromWire(spec.schema, body) as Record<string, unknown>;
    } catch (error) {
      if (!(error instanceof TypeError || error instanceof RangeError || error instanceof DOMException)) throw error;
      return new ServiceError(name!, stringField(body, "message") ?? `${operation.name} failed with HTTP ${response.status}`,
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
  return String(toWire(schema, value));
}

function headerValue(value: unknown, schema: Schema | undefined): string {
  if (!Array.isArray(value)) return scalar(value, schema);
  const member = schema?.kind === "list" ? schema.member : undefined;
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
): void {
  for (const [member, header, schema] of bindings ?? []) {
    const text = headers.get(header);
    if (text !== null) Object.defineProperty(into, member, { value: parseHeader(text, schema, wire), enumerable: true, writable: true, configurable: true });
  }
}

function parseHeader(text: string, schema: Schema | undefined, wire = false): unknown {
  switch (schema?.kind) {
    case "integer":
      return wire ? new JsonNumber(text.trim()) : fromWire(schema, new JsonNumber(text.trim()));
    case "number":
      return fromWire(schema, new JsonNumber(text.trim()));
    case "boolean":
      return text.trim() === "true";
    case "timestamp":
      return wire ? (schema.format === "epoch-seconds" ? new JsonNumber(text.trim()) : text)
        : parseTimestamp(schema.format === "epoch-seconds" ? Number(text) : text);
    case "list":
      return splitHeader(text).map(item => parseHeader(item, schema.member, wire));
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

function stringField(body: Record<string, unknown>, name: string): string | undefined {
  const value = body[name];
  return typeof value === "string" ? value : undefined;
}

function bearer(token: TokenProvider): Middleware {
  return async (request, next) => {
    const value = typeof token === "function" ? await token() : token;
    if (value) request.headers.set("authorization", `Bearer ${value}`);
    return next(request);
  };
}

/**
 * Adds `x-amz-content-sha256` to a request with a body: a front door that
 * signs requests to its origin needs the body's digest from the caller.
 */
export const contentSha256: Middleware = async (request, next) => {
  if (request.body === null || request.headers.has("x-amz-content-sha256")) return next(request);
  const bytes = await request.clone().arrayBuffer();
  const digest = new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", bytes));
  request.headers.set("x-amz-content-sha256", Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join(""));
  return next(request);
};

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
  const seconds = Number(response.headers.get("retry-after"));
  return Number.isFinite(seconds) && seconds > 0 ? seconds * 1000 : undefined;
}

function sleep(ms: number, signal: AbortSignal | undefined): Promise<void> {
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
