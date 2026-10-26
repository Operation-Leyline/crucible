// crucible TypeScript runtime. Generated clients carry a copy of this file
// and its companion modules, with no external runtime dependencies.
// Edit it in the crucible repository (crates/crucible-emit-ts/runtime/), never
// in a generated package.

/** Any JSON value: what a `document` holds. */
export type Document = null | boolean | number | bigint | string | Document[] | { [key: string]: Document };

/** An exact decimal JSON number. Construct from text to avoid floating-point rounding. */
export class Decimal {
  readonly #text: string;
  /** Validate exact JSON number text or construct from an integer without rounding. */
  constructor(value: string | bigint) {
    const text = String(value);
    if (!/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/.test(text)) throw new TypeError("invalid decimal");
    this.#text = text;
  }
  /** Return the original exact decimal text. */
  toString(): string { return this.#text; }
  /** Compare numeric values while ignoring equivalent exponent and zero spellings. */
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

/** A future union arm. `json` is its lossless JSON payload, not the enclosing union. */
export interface UnknownUnion {
  readonly $unknown: { readonly name: string; readonly json: string };
}

/** A union member or result that carries no value. On the wire it is `{}`. */
export type Unit = Record<string, never>;

/** Timestamp encoding selected by the lowered wire contract. */
export type TimestampFormat = "date-time" | "http-date" | "epoch-seconds";

/** Static wire conversion and validation metadata emitted from the lowered IR. */
export type Schema =
  | { readonly kind: "string" | "boolean" | "document" | "blob" | "bigInteger" | "bigDecimal" }
  | { readonly kind: "number" }
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
    };

/** Static schema for a string value. */
export const string: Schema = { kind: "string" };
/** Static schema for a floating-point value. */
export const number: Schema = { kind: "number" };
/** Static schema for a boolean value. */
export const boolean: Schema = { kind: "boolean" };
/** Static schema for arbitrary JSON document values. */
export const document: Schema = { kind: "document" };
/** Static schema for buffered bytes. */
export const blob: Schema = { kind: "blob" };
/** Static schema for arbitrary-precision integers. */
export const bigInteger: Schema = { kind: "bigInteger" };
/** Static schema for exact decimal values. */
export const bigDecimal: Schema = { kind: "bigDecimal" };
/** Create a signed integer schema with the specified bit width. */
export const integer = (bits: 8 | 16 | 32 | 64): Schema => ({ kind: "integer", bits });
/** Create a timestamp schema using the selected wire format. */
export const timestamp = (format: TimestampFormat): Schema => ({ kind: "timestamp", format });
/** Create a lazy list schema with sparse and uniqueness policies. */
export function list(member: Schema | (() => Schema), options: { sparse?: boolean; unique?: boolean } = {}): Schema {
  return { kind: "list", ...options, get member() { return typeof member === "function" ? member() : member; } };
}
/** Create a lazy map schema with an optional sparse-value policy. */
export function map(value: Schema | (() => Schema), options: { sparse?: boolean } = {}): Schema {
  return { kind: "map", ...options, get value() { return typeof value === "function" ? value() : value; } };
}

/** Thunks also support recursive list/map aliases without declaration-order work. */
export function structure(members: () => Record<string, Schema>, defaults?: Record<string, unknown>, required?: readonly string[]): Schema {
  let resolved: Record<string, Schema> | undefined;
  return { kind: "structure", get members() { return (resolved ??= members()); }, defaults, required };
}
/** Create a lazy union schema enforcing exactly one member; unknown wire members are preserved. */
export function union(members: () => Record<string, Schema>): Schema {
  let resolved: Record<string, Schema> | undefined;
  return { kind: "union", get members() { return (resolved ??= members()); } };
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

/**
 * Restore the JavaScript representation of JSON values. With `exact` (a
 * declared document), an integer token (digits only, no fraction or exponent)
 * beyond Number.isSafeInteger decodes as an exact bigint, however many digits
 * it has. Every other number, including "-0", exponent forms and fractions, is
 * a number, and a non-finite one (1e999) is an error. Without `exact` (an
 * undeclared member, which has no schema) every number stays a number.
 */
function jsonValue(value: unknown, exact = false): unknown {
  if (value instanceof JsonNumber) {
    if (exact && /^-?(0|[1-9]\d*)$/.test(value.text) && !Number.isSafeInteger(Number(value.text))) return BigInt(value.text);
    return jsonValue(Number(value.text));
  }
  if (typeof value === "number" && !Number.isFinite(value)) throw new TypeError("non-finite JSON number");
  if (value instanceof Uint8Array || value instanceof Date || value instanceof Decimal) return value;
  if (Array.isArray(value)) return value.map(item => jsonValue(item, exact));
  if (value !== null && typeof value === "object") return mapValues(value as Record<string, unknown>, item => jsonValue(item, exact));
  return value;
}
function object(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value) || value instanceof JsonNumber) {
    throw new TypeError("expected object");
  }
  return value as Record<string, unknown>;
}

/** Convert a model value to its lowered wire representation, validating required members and unions. */
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

/** Decode wire values into native model representations and preserve unknown union variants. */
export function fromWire(schema: Schema | undefined, value: unknown): unknown {
  if (schema === undefined || schema.kind === "document" || value === undefined || value === null) return jsonValue(value, schema?.kind === "document");
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
/** Clone public values while keeping exact decimals. */
function snapshotValue<T>(value: T, seen = new Map<object, unknown>()): T {
  if (value === null || typeof value !== "object") return value;
  if (value instanceof Decimal) return value;
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
  const names = Object.keys(value).filter(name => value[name] !== undefined);
  if (schema.kind === "union") {
    if (names.length !== 1) throw new TypeError("a union must have exactly one member");
    const name = names[0];
    if (convert === toWire && name === "$unknown") {
      const arm = object(value.$unknown);
      if (typeof arm.name !== "string" || typeof arm.json !== "string" || Object.hasOwn(schema.members, arm.name)) {
        throw new TypeError("unknown union arm must name an undeclared member and carry JSON text");
      }
      return Object.fromEntries([[arm.name, parseJson(arm.json)]]);
    }
    if (!Object.hasOwn(schema.members, name)) {
      if (convert === toWire) throw new TypeError("undeclared union arm requires $unknown");
      return { $unknown: { name, json: stringifyJson(value[name]) } };
    }
    return Object.fromEntries([[name, convert(schema.members[name], value[name])]]);
  }
  const native: Record<string, unknown> = Object.create(null);
  for (const name of Object.keys(value)) native[name] = value[name];
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
    return [name, convert(member, native[name])];
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

/**
 * Write converted model values, including exact unquoted bigint JSON numbers.
 * Unlike JSON.stringify, which throws on a bigint, this is safe for decoded
 * documents. `space` indents like JSON.stringify's (a count, at most 10, or a string).
 */
export function stringifyJson(value: unknown, space?: number | string): string {
  const gap = typeof space === "number" ? " ".repeat(Math.max(0, Math.min(10, Math.trunc(space)))) : (space ?? "").slice(0, 10);
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
    const parts = Array.isArray(item)
      ? Array.from(item, value => write(value === undefined ? null : value, depth + 1))
      : Object.entries(item).filter(([, value]) => value !== undefined)
        .map(([key, value]) => JSON.stringify(key) + (gap ? ": " : ":") + write(value, depth + 1));
    active.delete(item);
    const [open, close] = Array.isArray(item) ? ["[", "]"] : ["{", "}"];
    if (parts.length === 0) return open + close;
    if (!gap) return open + parts.join(",") + close;
    const inner = "\n" + gap.repeat(depth + 1);
    return open + inner + parts.join("," + inner) + "\n" + gap.repeat(depth) + close;
  };
  return write(value, 0);
}

/** Encode a Date using its modeled timestamp format. */
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

/** Decode standard base64 into bytes; invalid base64 throws. */
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

/** Decode timestamp text or epoch seconds to a Date; invalid timestamps throw. */
export function parseTimestamp(value: string | number): Date {
  const date = typeof value === "number" ? new Date(epochMillis(value)) : new Date(value);
  if (Number.isNaN(date.getTime())) throw new TypeError(`not a timestamp: ${String(value)}`);
  return date;
}

/** Where each member of an operation's input and output travels. */
export interface OperationSpec {
  /** Each item in a typed response sequence, using document encoding. */
  readonly stream?: Schema;
  /** The release this client was generated for, sent as `Crucible-Version` (docs/api-versions-clients.md). */
  readonly apiVersion?: string;
  readonly headerEncodings?: Readonly<Record<string,"base64">>;
  readonly name: string;
  /** Effective operation authentication; omitted only by older hand-written specs. */
  readonly bearer?: boolean;
  readonly method: string;
  /** Literal path segments and `{ label: member }` placeholders. */
  readonly path: readonly (string | { readonly label: string; readonly schema?: Schema; readonly greedy?: boolean })[];
  readonly literalQuery?: readonly (readonly [string, string | undefined])[];
  readonly query?: readonly (readonly [member: string, param: string, schema?: Schema])[];
  readonly headers?: readonly (readonly [member: string, header: string, schema?: Schema])[];
  /** Input members in the JSON body; absent when the request has no body. */
  readonly body?: readonly string[];
  /** The input member that is the whole body, raw (`@httpPayload`), and its content type. */
  readonly payload?: { readonly member: string; readonly mediaType: string; readonly kind?: "blob" | "string" | "document" };
  readonly input?: Schema;
  readonly output?: {
    readonly body: boolean;
    /** The output member that is the whole body, read as bytes. */
    readonly payload?: string;
    readonly payloadKind?: "blob" | "string" | "document";
    readonly headerEncodings?: Readonly<Record<string,"base64">>;
    readonly schema?: Schema;
    readonly headers?: readonly (readonly [member: string, header: string, schema?: Schema])[];
  };
  /** The errors this operation can return, by shape name. */
  readonly errors: readonly string[];
  readonly idempotencyToken?: string;
  /** Repeating the request cannot repeat its effect: readonly, idempotent or tokened. */
  readonly retrySafe: boolean;
}

/** HTTP status, request identifier and headers retained from the response. */
export interface ResponseMetadata {
  /** HTTP response status code. */
  readonly status: number;
  /** Service request identifier when present. */
  readonly requestId?: string;
  /** HTTP response headers. */
  readonly headers: Headers;
}

/** An error response the service sent. Each modelled error has its own subclass. */
export class ServiceError extends Error {
  /** Status, request identifier and headers from the service response. */
  readonly $metadata: ResponseMetadata;
  /** Modeled client/server fault classification. */
  readonly $fault: "client" | "server";
  /** Modeled retryability and throttling classification, when declared. */
  readonly $retryable?: { readonly throttling: boolean };
  // BEGIN ERROR DOCUMENTATION
  /** Static documentation page for the modeled error code and known reason. */
  readonly docUrl?: string;
  // END ERROR DOCUMENTATION

  /** Construct an error response with its identity, message, metadata and retry classification. */
  constructor(
    name: string,
    message: string,
    metadata: ResponseMetadata,
    fault: "client" | "server",
    retryable?: { readonly throttling: boolean },
    // BEGIN ERROR DOCUMENTATION
    docUrl?: string,
    // END ERROR DOCUMENTATION
  ) {
    super(message);
    this.name = name;
    this.$metadata = metadata;
    this.$fault = fault;
    this.$retryable = retryable;
    // BEGIN ERROR DOCUMENTATION
    this.docUrl = docUrl;
    // END ERROR DOCUMENTATION
  }
}

/** Base for every error response from this service. */
export class ServiceException extends ServiceError {}

// BEGIN ERROR DOCUMENTATION
/** Precomputed pages from the audience-projected model. */
export interface ErrorDocumentation {
  readonly url: string;
  readonly reasonMember: string | null;
  readonly reasonDefault?: string;
  readonly reasonUrls: Readonly<Record<string, string>>;
}
/** Select declared pages from the effective value; only absence applies the model default. */
export function errorDocUrl(documentation: ErrorDocumentation, fields: Record<string, unknown>): string {
  const value = documentation.reasonMember === null || !Object.hasOwn(fields, documentation.reasonMember)
    ? undefined : fields[documentation.reasonMember];
  const reason = value === undefined ? documentation.reasonDefault : value;
  return typeof reason === "string" && Object.hasOwn(documentation.reasonUrls, reason)
    ? documentation.reasonUrls[reason] : documentation.url;
}
// END ERROR DOCUMENTATION

type DebugMember = { readonly target: string; readonly attribute: string; readonly sensitive?: boolean };
type DebugShape = { readonly name: string; readonly kind: string; readonly sensitive?: boolean; readonly members?: Readonly<Record<string, DebugMember>>; readonly member?: DebugMember; readonly key?: DebugMember; readonly value?: DebugMember };

/** Format modeled error fields; omit undefined, preserve null and redact unknown union data. */
export function formatModel(shapes: Readonly<Record<string, DebugShape>>, target: string, value: unknown): string {
  const active = new Set<object>();
  const render = (target: string, value: unknown, sensitive = false): string => {
    const shape = shapes[target];
    if (sensitive || shape.sensitive) return "*** Sensitive Data Redacted ***";
    if (value === null) return "null";
    if (value === undefined) return "undefined";
    if (typeof value !== "object") return typeof value === "string" ? JSON.stringify(value) : String(value);
    if (active.has(value)) return "<recursive>";
    active.add(value);
    try {
      if (shape.kind === "structure" || shape.kind === "union") {
        const fields = value as Record<string, unknown>;
        const parts = Object.values(shape.members ?? {}).filter(member => Object.hasOwn(fields, member.attribute) && fields[member.attribute] !== undefined).map(member => `${member.attribute}=${render(member.target, fields[member.attribute], member.sensitive)}`);
        if (shape.kind === "union" && fields.$unknown !== undefined) parts.push("$unknown=*** Sensitive Data Redacted ***");
        return `${shape.name}(${parts.join(", ")})`;
      }
      if (shape.kind === "list") return `[${(value as unknown[]).map(item => render(shape.member!.target, item, shape.member!.sensitive)).join(", ")}]`;
      if (shape.kind === "map") return `{${Object.entries(value).map(([key, item]) => `${render(shape.key!.target, key, shape.key!.sensitive)}: ${render(shape.value!.target, item, shape.value!.sensitive)}`).join(", ")}}`;
      if (value instanceof Date) return value.toISOString();
      if (value instanceof Decimal) return value.toString();
      return stringifyJson(value);
    } finally { active.delete(value); }
  };
  return render(target, value);
}

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

/** Static modeled error decoding metadata and its typed constructor. */
export interface ErrorSpec {
  // BEGIN ERROR DOCUMENTATION
  readonly documentation?: ErrorDocumentation;
  // END ERROR DOCUMENTATION
  readonly payload?: string;
  readonly payloadKind?: "blob" | "string" | "document";
  readonly headerEncodings?: Readonly<Record<string,"base64">>;
  readonly status: number;
  readonly fault?: "client" | "server";
  readonly retryable?: { readonly throttling: boolean };
  readonly schema?: Schema;
  readonly headers?: readonly (readonly [member: string, header: string, schema?: Schema])[];
  readonly create: (fields: Record<string, unknown>, metadata: ResponseMetadata) => ServiceError;
}

/** HTTP transport receiving a Request and returning a Response. Defaults to global fetch. */
export type Fetch = (input: Request) => Promise<Response>;
/** The next middleware or fetch transport in the request chain. */
export type Next = (request: Request) => Promise<Response>;
/** Sees every attempt of every request, in order, before it is sent. */
export type Middleware = (request: Request, next: Next) => Promise<Response>;
/** A bearer token or a provider invoked for each request attempt; undefined means unavailable. */
export type TokenProvider = string | (() => string | undefined | Promise<string | undefined>);

/** Exponential full-jitter retry delays and the total attempt limit. Only retry-safe calls are retried. */
export interface RetryConfig {
  /** Attempts in all, the first included. */
  readonly maxAttempts?: number;
  /** Initial full-jitter backoff ceiling in milliseconds; defaults to 100. */
  readonly baseDelayMs?: number;
  /** Maximum retry backoff delay in milliseconds; defaults to 20,000. */
  readonly maxDelayMs?: number;
}

/** Configuration shared by every call; construction validates endpoint and retry settings. */
export interface ClientConfig {
  /** Maximum encoded SSE frame size; defaults to 1 MiB. */
  readonly maxStreamEventBytes?: number;
  /** Absolute HTTP(S) base URL; if omitted, the generated default is used. */
  readonly endpoint?: string;
  /** Sent as a bearer token. A function is called for every attempt, so a refreshed token is picked up. */
  readonly token?: TokenProvider;
  /** Custom fetch transport; otherwise global fetch is used. */
  readonly fetch?: Fetch;
  /** Headers sent on every request; a body's or payload's Content-Type takes precedence. */
  readonly headers?: Readonly<Record<string, string>>;
  /** Runs after the client's own middleware, nearest the network last. */
  readonly middleware?: readonly Middleware[];
  /** Retry limits and backoff; false disables retries. */
  readonly retry?: RetryConfig | false;
  /** Total call budget, including credentials, middleware, attempts and response bodies. */
  readonly timeoutMs?: number;
}

/** Per-call cancellation, total timeout and header overrides. */
export interface RequestOptions {
  /** Overrides the response stream's encoded frame size limit. */
  readonly maxStreamEventBytes?: number;
  /** Overrides the client budget; null disables it for this call. */
  readonly timeoutMs?: number | null;
  /** Abort this call when the signal is aborted. */
  readonly signal?: AbortSignal;
  /** Additional HTTP headers; per-call values override client headers, but not a body's or payload's Content-Type. */
  readonly headers?: Readonly<Record<string, string>>;
}

const RETRYABLE_STATUS = new Set([500, 502, 503, 504]);

/** Invalid framing or a connection ending before a terminal outcome. */
export class StreamProtocolError extends Error {
  constructor(message: string) { super(message); this.name = "StreamProtocolError"; }
}

/** A single-consumer stream. `break`, cancel and abort release its transport. */
export class ResponseStream<T> implements AsyncIterableIterator<T> {
  readonly metadata: ResponseMetadata;
  readonly #iterator: AsyncGenerator<T>;
  readonly #close: (reason?: unknown) => Promise<void>;
  #claimed = false;
  #pending = false;
  constructor(metadata: ResponseMetadata, iterator: AsyncGenerator<T>, close: (reason?: unknown) => Promise<void>) {
    this.metadata = metadata; this.#iterator = iterator; this.#close = close;
  }
  [Symbol.asyncIterator](): AsyncIterableIterator<T> {
    if (this.#claimed) throw new TypeError("response streams have one consumer");
    this.#claimed = true; return this;
  }
  async next(): Promise<IteratorResult<T>> {
    if (this.#pending) throw new TypeError("concurrent stream reads are unsupported");
    this.#pending = true;
    try { return await this.#iterator.next(); }
    catch (error) { await this.#close(error); throw error; }
    finally { this.#pending = false; }
  }
  async return(): Promise<IteratorResult<T>> {
    await this.#close();
    return this.#iterator.return(undefined);
  }
  async cancel(reason?: unknown): Promise<void> { await this.#close(reason); }
}

/** One lifetime owns credentials, opening and consumption, not just headers. */
function callLifetime(config: ClientConfig, options: RequestOptions): { signal: AbortSignal; close: () => void; abort: (reason?: unknown) => void } {
  const timeout = options.timeoutMs === undefined ? config.timeoutMs : options.timeoutMs;
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
  return { signal: controller.signal, abort: reason => controller.abort(reason), close: () => {
    if (timer !== undefined) clearTimeout(timer);
    options.signal?.removeEventListener("abort", abort);
  } };
}

/** Incremental standard SSE framing; terminal semantics belong to Crucible. */
async function* streamEvents<T>(reader: ReadableStreamDefaultReader<Uint8Array>, signal: AbortSignal, decode: (event: string, data: unknown) => T, close: () => Promise<void>, limit: number): AsyncGenerator<T> {
  const utf8 = new TextDecoder("utf-8", { fatal: true });
  let buffer = "", event = "", data = "", terminal = false, eof = false, frameBytes = 0, scan = 0, lineBytes = 0;
  try {
    while (!eof) {
      let chunk: ReadableStreamReadResult<Uint8Array>;
      try { chunk = await abortable(reader.read(), signal); }
      catch (error) { if (isAbort(error, signal)) throw error; throw new NetworkError(error); }
      eof = chunk.done;
      try { buffer += utf8.decode(chunk.value, { stream: !eof }); }
      catch { throw new StreamProtocolError("invalid UTF-8 in response stream"); }
      let start = 0;
      let index = scan;
      for (; index < buffer.length; index++) {
        const char = buffer[index];
        if (char !== "\r" && char !== "\n") {
          const code = buffer.charCodeAt(index);
          lineBytes += code < 0x80 ? 1 : code < 0x800 ? 2 : code >= 0xdc00 && code <= 0xdfff ? 0 : code >= 0xd800 && code <= 0xdbff ? 4 : 3;
          if (frameBytes + lineBytes > limit) throw new StreamProtocolError("stream event exceeds the byte limit");
          continue;
        }
        if (char === "\r" && index + 1 === buffer.length && !eof) break;
        const line = buffer.slice(start, index);
        if (terminal) throw new StreamProtocolError("data after terminal stream outcome");
        frameBytes += lineBytes + (char === "\r" && buffer[index + 1] === "\n" ? 2 : 1);
        lineBytes = 0;
        if (frameBytes > limit) throw new StreamProtocolError("stream event exceeds the byte limit");
        if (char === "\r" && buffer[index + 1] === "\n") index++;
        start = index + 1;
        if (line === "") {
          if (data !== "") {
            signal.throwIfAborted();
            if (terminal) throw new StreamProtocolError("data after terminal stream outcome");
            const kind = event || "message";
            let value: unknown;
            try { value = parseJson(data.slice(0, -1)); }
            catch { throw new StreamProtocolError("invalid JSON in response stream"); }
            if (kind === "crucible.complete") {
              if (value === null || typeof value !== "object" || Array.isArray(value) || Object.keys(value).length !== 0) throw new StreamProtocolError("invalid stream completion");
              terminal = true;
            } else if (kind === "crucible.error") {
              decode(kind, value); // always throws a typed service error
              throw new StreamProtocolError("invalid terminal error");
            } else if (kind === "message") {
              let item: T;
              try { item = decode(kind, value); }
              catch { throw new StreamProtocolError("invalid modeled stream item"); }
              yield item;
            }
            else throw new StreamProtocolError("unsupported stream event");
          }
          event = ""; data = ""; frameBytes = 0;
        } else if (!line.startsWith(":")) {
          const colon = line.indexOf(":");
          const field = colon < 0 ? line : line.slice(0, colon);
          let value = colon < 0 ? "" : line.slice(colon + 1);
          if (value.startsWith(" ")) value = value.slice(1);
          if (field === "event") event = value;
          else if (field === "data") data += value + "\n";
        }
        if (data.length + event.length > limit) throw new StreamProtocolError("stream event exceeds the limit");
      }
      scan = index - start;
      buffer = buffer.slice(start);
      if (buffer.length + data.length > limit) throw new StreamProtocolError("stream event exceeds the limit");
    }
    if (!terminal || buffer.trim() !== "" || data !== "") throw new StreamProtocolError("response stream ended without a complete terminal outcome");
  } finally { await close(); }
}

/** Execute modeled operations with authentication, middleware, retries and a total deadline. */
export class Transport {
  readonly #config: ClientConfig;
  readonly #endpoint: string;
  readonly #middleware: readonly Middleware[];
  readonly #bearer: boolean;
  readonly #errors: Readonly<Record<string, ErrorSpec>>;

  constructor(
    config: ClientConfig,
    defaults: { readonly endpoint?: string; readonly middleware: readonly Middleware[]; readonly bearer: boolean },
    errors: Readonly<Record<string, ErrorSpec>>,
  ) {
    const endpoint = config.endpoint ?? defaults.endpoint;
    if (!endpoint) throw new TypeError("no endpoint: pass `endpoint` in the client config");
    if (!validEndpoint(endpoint)) throw new TypeError("endpoint must be an absolute HTTP(S) URL without credentials, query, or fragment");
    this.#config = config;
    this.#endpoint = endpoint.replace(/\/+$/, "");
    this.#errors = errors;
    this.#bearer = defaults.bearer;
    this.#middleware = [...defaults.middleware, ...(config.middleware ?? [])];
  }

  /** Opens once; success is determined while iterating, not when headers arrive. */
  async stream<T>(operation: OperationSpec, input: object, options: RequestOptions = {}): Promise<ResponseStream<T>> {
    if (operation.stream === undefined) throw new TypeError("operation is not streaming");
    const limit = options.maxStreamEventBytes ?? this.#config.maxStreamEventBytes ?? 1024 * 1024;
    if (!Number.isSafeInteger(limit) || limit < 256) throw new TypeError("maxStreamEventBytes must be an integer of at least 256");
    const lifetime = callLifetime(this.#config, options);
    let reader: ReadableStreamDefaultReader<Uint8Array> | undefined;
    let closed = false;
    const close = async (reason?: unknown) => {
      if (closed) return;
      closed = true; lifetime.abort(reason); lifetime.close();
      try { await reader?.cancel(reason); } catch { /* cancellation is best effort */ }
      reader?.releaseLock();
    };
    try {
      const response = await abortable(this.#send<Response>(operation, input, { ...options, signal: lifetime.signal }), lifetime.signal);
      const contentType = response.headers.get("content-type") ?? "";
      if (contentType.split(";", 1)[0].trim().toLowerCase() !== "text/event-stream" || !/;\s*crucible-stream\s*=\s*(?:"1"|1)\s*(?:;|$)/i.test(contentType) || !response.body) {
        await response.body?.cancel(); throw new StreamProtocolError("unsupported streaming response");
      }
      reader = response.body.getReader();
      const metadata: ResponseMetadata = { status: response.status, headers: response.headers, requestId: response.headers.get("x-request-id") ?? response.headers.get("x-amzn-requestid") ?? response.headers.get("x-amz-request-id") ?? undefined };
      const decode = (kind: string, value: unknown): T => {
        if (kind === "message") {
          if (value === null || value === undefined) throw new TypeError("stream items cannot be null");
          return fromWire(operation.stream, value) as T;
        }
        let error: ServiceException;
        try {
          const envelope = object(value);
          const status = fromWire(integer(32), envelope.status) as number;
          if (typeof envelope.type !== "string" || envelope.type.length === 0 || typeof status !== "number" || status < 400 || status > 599) throw new Error();
          const fields = object(envelope.error);
          const errorMetadata = { ...metadata, status };
          const spec = operation.errors.includes(envelope.type) && Object.hasOwn(this.#errors, envelope.type) ? this.#errors[envelope.type] : undefined;
          error = spec ? spec.create(fromWire(spec.schema, fields) as Record<string, unknown>, errorMetadata)
            : new ServiceException(envelope.type, stringField(fields, "message") ?? "Stream failed", errorMetadata, status >= 500 ? "server" : "client");
        } catch { throw new StreamProtocolError("invalid terminal stream error"); }
        throw error;
      };
      const abortReader = () => { void close(lifetime.signal.reason); };
      lifetime.signal.addEventListener("abort", abortReader, { once: true });
      const cleanup = async () => { lifetime.signal.removeEventListener("abort", abortReader); await close(); };
      return new ResponseStream(metadata, streamEvents(reader, lifetime.signal, decode, cleanup, limit), close);
    } catch (error) { await close(error); throw error; }
  }

  /** Execute one logical call with configured retries, per-call overrides and a total deadline. */
  async send<O>(operation: OperationSpec, input: object, options: RequestOptions = {}): Promise<O> {
    if (operation.stream !== undefined) throw new TypeError("use stream() for a streaming operation");
    const lifetime = callLifetime(this.#config, options);
    try {
      lifetime.signal.throwIfAborted();
      return await abortable(this.#send<O>(operation, input, { ...options, signal: lifetime.signal }), lifetime.signal);
    } finally { lifetime.close(); }
  }

  async #send<O>(operation: OperationSpec, input: object, options: RequestOptions): Promise<O> {
    // Preserve write-back of generated tokens, but replay one logical input even
    // when the caller changes its mutable objects while a request is in flight.
    const values = snapshotValue(withIdempotencyToken(operation, input)) as Record<string, unknown>;
    const retry = this.#config.retry === false ? { maxAttempts: 1 } : (this.#config.retry ?? {});
    const maxAttempts = Math.max(1, retry.maxAttempts ?? 3);
    for (let attempt = 1; ; attempt++) {
      // Invalid inputs are caller errors, not transport failures.
      options.signal?.throwIfAborted();
      let request = this.#request(operation, values, options);
      if (operation.stream !== undefined) request.headers.set("accept", "text/event-stream; crucible-stream=1");
      if (operation.bearer ?? this.#bearer) {
        try { request = await authenticate(request, this.#config); }
        catch (error) { await request.body?.cancel(error).catch(() => {}); throw error; }
      }
      let response: Response | undefined;
      let text = "";
      let bytes: Uint8Array | undefined;
      try {
        response = await this.#dispatch(request);
        if (options.signal?.aborted) {
          void response.body?.cancel().catch(() => {});
          options.signal.throwIfAborted();
        }
        // Fetch resolves at the headers; required bodies can still fail.
        // A successful operation without a modeled body needs only headers;
        // a payload is read as the bytes sent, never as text.
        if (response.ok && operation.stream !== undefined) {
          // Reader ownership and the deadline pass to ResponseStream.
        } else if (response.ok && operation.output?.payload !== undefined) {
          bytes = await readResponse(response, options.signal);
        } else if (!response.ok || operation.output?.body) {
          bytes = await readResponse(response, options.signal);text = new TextDecoder().decode(bytes);
        }
      } catch (error) {
        if (isAbort(error, options.signal)) throw error;
        const network = new NetworkError(error);
        if (!operation.retrySafe || attempt >= maxAttempts) throw network;
        const delay = response === undefined ? undefined : retryAfter(response);
        await sleep(delay ?? backoff(attempt, retry), options.signal);
        continue;
      }
      if (response.ok) return (operation.stream !== undefined ? response : this.#decodeOutput(operation, response, text, bytes)) as O;
      const error = this.#decodeError(operation, response, text, bytes ?? new Uint8Array());
      const throttled = error.$retryable?.throttling === true;
      const transient = error.$retryable !== undefined || RETRYABLE_STATUS.has(response.status);
      if (!(throttled || (operation.retrySafe && transient)) || attempt >= maxAttempts) throw error;
      await sleep(retryAfter(response) ?? backoff(attempt, retry), options.signal);
    }
  }

  #dispatch(request: Request): Promise<Response> {
    const send = this.#config.fetch ?? ((request: Request) => fetch(request));
    const signal = request.signal;
    const checked: Next = async pending => {
      signal.throwIfAborted(); pending.signal.throwIfAborted();
      // Also over a request a middleware rebuilt without the redirect mode.
      return send(new Request(pending, { signal: AbortSignal.any([signal, pending.signal]), redirect: "manual" }));
    };
    const chain = this.#middleware.reduceRight<Next>((next, middleware) => request => {
      signal.throwIfAborted(); request.signal.throwIfAborted(); return middleware(request, next);
    }, checked);
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
        const text = scalar(value, segment.schema);
        // URL and Fetch normalize even percent-encoded dot segments. Never
        // silently change the requested resource when it cannot be preserved.
        const segments = segment.greedy ? text.split("/") : [text];
        if (segments.some(part => part === "." || part === "..")) throw new TypeError(`${operation.name}: dot-segment label \`${segment.label}\` is unsupported`);
        return segments.map(encodeURIComponent).join("/");
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
    const headers = new Headers();
    for (const [name, value] of Object.entries({ ...this.#config.headers, ...options.headers })) headers.set(name, value);
    // A Content-Type bound to an input member outranks the payload's media
    // type, which outranks client and per-call header defaults.
    let modelledContentType = false;
    for (const [member, header, schema] of operation.headers ?? []) {
      const value = values[member];
      if (value === undefined || value === null) continue;
      headers.set(header, operation.headerEncodings?.[member]==="base64"?(Array.isArray(value)?value.map(encodeMedia).join(", "):encodeMedia(value)):headerValue(value, schema));
      if (header.toLowerCase() === "content-type") modelledContentType = true;
    }
    let body: BodyInit | undefined;
    if (operation.body !== undefined) {
      const picked: Record<string, unknown> = Object.create(null);
      for (const member of operation.body) {
        if (encoded[member] !== undefined) picked[member] = encoded[member];
      }
      body = stringifyJson(picked);
      // A protocol document keeps the media type of its encoding.
      headers.set("content-type", "application/json");
    }
    if (operation.payload !== undefined) {
      const value = values[operation.payload.member];
      // JSON conversion owns null-member presence; raw payloads bypass member conversion.
      const payloadValue = operation.payload.kind === "document"
        ? encoded[operation.payload.member] : value;
      if (payloadValue !== undefined && (value !== null || operation.payload.kind==="document")) {
        if (operation.payload.kind==="string") {
          if (typeof value!=="string") throw new TypeError(`${operation.name}: payload must be a string`);body=value;
        } else if (operation.payload.kind==="document") {
          body=stringifyJson(payloadValue);
        } else {
          if (!(value instanceof Uint8Array)) throw new TypeError(`${operation.name}: payload must be a Uint8Array`);
          body = value as BodyInit;
        }
        if (!modelledContentType) headers.set("content-type", operation.payload.mediaType);
      }
    }
    if (operation.apiVersion !== undefined) headers.set("crucible-version", operation.apiVersion);
    // A followed redirect would resend credential headers to wherever the
    // Location points; the 3xx is returned to the caller instead.
    return new Request(url, { method: operation.method, headers, body, signal: options.signal, redirect: "manual" });
  }

  #decodeOutput(operation: OperationSpec, response: Response, text: string, bytes: Uint8Array | undefined): unknown {
    const output = operation.output;
    if (output === undefined) return {};
    const body = output.payload !== undefined
      ? { [output.payload]: decodePayload(output.payloadKind,bytes ?? new Uint8Array()) }
      : output.body ? readJson(text) : {};
    const fields = object(body ?? {});
    readHeaders(response.headers, output.headers, fields, true, output.headerEncodings);
    return fromWire(output.schema, fields);
  }

  #decodeError(operation: OperationSpec, response: Response, text: string, bytes: Uint8Array): ServiceError {
    let parsed: unknown;
    try { parsed = readJson(text); } catch { /* An HTTP error may have a non-JSON body. */ }
    // BEGIN ERROR DOCUMENTATION
    const documentationBodyValid = !/[^ \t\r\n]/.test(text) || (parsed !== null && typeof parsed === "object" && !Array.isArray(parsed));
    // END ERROR DOCUMENTATION
    const body = (parsed !== null && typeof parsed === "object" ? parsed : {}) as Record<string, unknown>;
    const metadata: ResponseMetadata = {
      status: response.status,
      requestId: response.headers.get("x-request-id") ?? response.headers.get("x-amzn-requestid") ?? response.headers.get("x-amz-request-id") ?? stringField(body, "requestId"),
      headers: response.headers,
    };
    const name=response.headers.get("x-crucible-error") ?? errorName(body,operation,this.#errors,response.status);
    const spec = name !== undefined && operation.errors.includes(name) && Object.hasOwn(this.#errors, name)
      ? this.#errors[name] : undefined;
    if (spec === undefined) {
      const message = stringField(body, "message") ?? `${operation.name} failed with HTTP ${response.status}`;
      return new ServiceException(name ?? "UnknownError", message, metadata, response.status >= 500 ? "server" : "client");
    }
    let fields: Record<string, unknown>;
    try {
      // BEGIN ERROR DOCUMENTATION
      if (spec.payload === undefined && !documentationBodyValid) throw new TypeError("modeled error body requires an object");
      // END ERROR DOCUMENTATION
      if (spec.payload!==undefined) Object.defineProperty(body,spec.payload,{value:decodePayload(spec.payloadKind,bytes),writable:true,configurable:true,enumerable:true});
      readHeaders(response.headers, spec.headers, body, true, spec.headerEncodings);
      fields = fromWire(spec.schema, body) as Record<string, unknown>;
    } catch (error) {
      if (!(error instanceof TypeError || error instanceof RangeError || error instanceof DOMException)) throw error;
      return new ServiceException(name!, stringField(body, "message") ?? `${operation.name} failed with HTTP ${response.status}`,
        metadata, spec.fault ?? (response.status >= 500 ? "server" : "client"), spec.retryable
        // BEGIN ERROR DOCUMENTATION
        , spec.documentation?.url
        // END ERROR DOCUMENTATION
      );
    }
    return spec.create(fields, metadata);
  }
}

/** An absolute HTTP(S) URL with a plain host and no credentials, query or fragment. */
function validEndpoint(value: string): boolean {
  if (/[\x00-\x20\x7f?#\\]/.test(value) || !/^https?:\/\//.test(value)) return false;
  try {
    const url = new URL(value);
    if (url.username || url.password) return false;
    const ip = /^\d+\.\d+\.\d+\.\d+$/.test(url.hostname) || url.hostname.startsWith("[");
    return ip || url.hostname.replace(/\.$/, "").split(".").every(label => /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(label));
  } catch { return false; }
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
  encodings?: Readonly<Record<string,"base64">>,
): void {
  for (const [member, header, schema] of bindings ?? []) {
    const text = headers.get(header);
    if (text !== null) Object.defineProperty(into, member, { value: encodings?.[member]==="base64"?(schema?.kind==="list"?splitHeader(text).map(decodeMedia):decodeMedia(text)):parseHeader(text, schema, wire), enumerable: true, writable: true, configurable: true });
  }
}

function parseHeader(text: string, schema: Schema | undefined, wire = false): unknown {
  switch (schema?.kind) {
    case "integer": case "bigInteger": case "bigDecimal":
      return wire ? new JsonNumber(text.trim()) : fromWire(schema, new JsonNumber(text.trim()));
    case "number": {
      const trimmed = text.trim();
      const value = new JsonNumber(trimmed);
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
function decodePayload(kind: "blob" | "string" | "document" | undefined, bytes: Uint8Array): unknown {
  if (kind===undefined || kind==="blob") return bytes;
  const text=new TextDecoder("utf-8",{fatal:true}).decode(bytes);
  return kind==="string"?text:readJson(text);
}

function stringField(body: Record<string, unknown>, name: string): string | undefined {
  const value = body[name];
  return typeof value === "string" ? value : undefined;
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

/** Sets the bearer token, unless the caller already supplied one. */
async function authenticate(request: Request, config: ClientConfig): Promise<Request> {
  // An explicit bearer credential takes precedence over the provider.
  if (/^Bearer\s+\S/i.test(request.headers.get("authorization") ?? "")) return request;
  const token = typeof config.token === "function" ? await config.token() : config.token;
  request.signal.throwIfAborted();
  // The service decides whether an anonymous request is permitted.
  if (token) request.headers.set("authorization", `Bearer ${token}`);
  return request;
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

/** Race an asynchronous task against cancellation and release a late result when requested. */
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

/** Wait for the given milliseconds or reject on cancellation. */
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
