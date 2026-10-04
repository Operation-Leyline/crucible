// crucible TypeScript runtime. Generated clients carry a copy of this file
// and import nothing else, so a client and its runtime can never disagree.
// Edit it in the crucible repository (crates/crucible-emit-ts/runtime/), never
// in a generated package.
export const string = { kind: "string" };
export const number = { kind: "number" };
export const boolean = { kind: "boolean" };
export const timestamp = (format) => ({ kind: "timestamp", format });
export const list = (member) => ({ kind: "list", member });
export const map = (value) => ({ kind: "map", value });
/** Members are a thunk, so recursive and forward references resolve lazily. */
export function structure(members, defaults) {
    let resolved;
    return {
        kind: "structure",
        get members() {
            return (resolved ??= members());
        },
        defaults,
    };
}
export function toWire(schema, value) {
    if (schema === undefined || value === undefined || value === null)
        return value;
    switch (schema.kind) {
        case "timestamp":
            return formatTimestamp(value, schema.format);
        case "list":
            return value.map(item => toWire(schema.member, item));
        case "map":
            return mapValues(value, item => toWire(schema.value, item));
        case "structure":
            return convertMembers(schema, value, toWire);
        default:
            return value;
    }
}
export function fromWire(schema, value) {
    if (schema === undefined || value === undefined || value === null)
        return value;
    switch (schema.kind) {
        case "timestamp":
            return parseTimestamp(value);
        case "list":
            return value.map(item => fromWire(schema.member, item));
        case "map":
            return mapValues(value, item => fromWire(schema.value, item));
        case "structure": {
            const out = convertMembers(schema, value, fromWire);
            for (const [name, fallback] of Object.entries(schema.defaults ?? {})) {
                if (out[name] === undefined)
                    out[name] = structuredClone(fallback);
            }
            return out;
        }
        default:
            return value;
    }
}
function convertMembers(schema, value, convert) {
    const out = { ...value };
    for (const [name, member] of Object.entries(schema.members)) {
        if (name in out)
            out[name] = convert(member, out[name]);
    }
    return out;
}
function mapValues(value, convert) {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, convert(item)]));
}
export function formatTimestamp(value, format) {
    switch (format) {
        case "date-time":
            return value.toISOString();
        case "http-date":
            return value.toUTCString();
        case "epoch-seconds":
            return value.getTime() / 1000;
    }
}
export function parseTimestamp(value) {
    const date = typeof value === "number" ? new Date(value * 1000) : new Date(value);
    if (Number.isNaN(date.getTime()))
        throw new TypeError(`not a timestamp: ${String(value)}`);
    return date;
}
/** An error response the service sent. Each modelled error has its own subclass. */
export class ServiceError extends Error {
    $metadata;
    $fault;
    $retryable;
    constructor(name, message, metadata, fault, retryable) {
        super(message);
        this.name = name;
        this.$metadata = metadata;
        this.$fault = fault;
        this.$retryable = retryable;
    }
}
/** The request never got a response: DNS, connection, TLS or CORS. */
export class NetworkError extends Error {
    cause;
    constructor(cause) {
        super(cause instanceof Error ? cause.message : String(cause));
        this.cause = cause;
        this.name = "NetworkError";
    }
}
const RETRYABLE_STATUS = new Set([500, 502, 503, 504]);
export class Transport {
    #config;
    #endpoint;
    #middleware;
    #errors;
    constructor(config, defaults, errors) {
        const endpoint = config.endpoint ?? defaults.endpoint;
        if (!endpoint)
            throw new TypeError("no endpoint: pass `endpoint` in the client config");
        this.#config = config;
        this.#endpoint = endpoint.replace(/\/+$/, "");
        this.#errors = errors;
        const auth = defaults.bearer && config.token !== undefined ? [bearer(config.token)] : [];
        this.#middleware = [...auth, ...defaults.middleware, ...(config.middleware ?? [])];
    }
    async send(operation, input, options = {}) {
        const values = withIdempotencyToken(operation, input);
        const retry = this.#config.retry === false ? { maxAttempts: 1 } : (this.#config.retry ?? {});
        const maxAttempts = Math.max(1, retry.maxAttempts ?? 3);
        for (let attempt = 1;; attempt++) {
            let response;
            try {
                response = await this.#dispatch(this.#request(operation, values, options));
            }
            catch (error) {
                if (isAbort(error, options.signal))
                    throw error;
                const network = new NetworkError(error);
                if (!operation.retrySafe || attempt >= maxAttempts)
                    throw network;
                await sleep(backoff(attempt, retry), options.signal);
                continue;
            }
            if (response.ok)
                return (await this.#decodeOutput(operation, response));
            const error = await this.#decodeError(operation, response);
            const throttled = error.$retryable?.throttling === true;
            const transient = error.$retryable !== undefined || RETRYABLE_STATUS.has(response.status);
            if (!(throttled || (operation.retrySafe && transient)) || attempt >= maxAttempts)
                throw error;
            await sleep(retryAfter(response) ?? backoff(attempt, retry), options.signal);
        }
    }
    #dispatch(request) {
        const send = this.#config.fetch ?? ((request) => fetch(request));
        const chain = this.#middleware.reduceRight((next, middleware) => request => middleware(request, next), send);
        return chain(request);
    }
    #request(operation, values, options) {
        const path = operation.path
            .map(segment => {
            if (typeof segment === "string")
                return segment;
            const value = values[segment.label];
            if (value === undefined || value === null || value === "") {
                throw new TypeError(`${operation.name}: \`${segment.label}\` is required`);
            }
            return encodeURIComponent(scalar(value, segment.schema));
        })
            .join("/");
        const url = new URL(`${this.#endpoint}/${path}`);
        for (const [key, value] of operation.literalQuery ?? [])
            url.searchParams.append(key, value ?? "");
        for (const [member, param, schema] of operation.query ?? []) {
            const value = values[member];
            if (value === undefined || value === null)
                continue;
            const items = Array.isArray(value) ? value : [value];
            const itemSchema = schema?.kind === "list" ? schema.member : schema;
            for (const item of items)
                url.searchParams.append(param, scalar(item, itemSchema));
        }
        const headers = new Headers({ ...this.#config.headers, ...options.headers });
        for (const [member, header, schema] of operation.headers ?? []) {
            const value = values[member];
            if (value !== undefined && value !== null)
                headers.set(header, headerValue(value, schema));
        }
        let body;
        if (operation.body !== undefined) {
            const picked = {};
            for (const member of operation.body)
                if (values[member] !== undefined)
                    picked[member] = values[member];
            body = JSON.stringify(toWire(operation.input, picked));
            headers.set("content-type", "application/json");
        }
        return new Request(url, { method: operation.method, headers, body, signal: options.signal });
    }
    async #decodeOutput(operation, response) {
        const output = operation.output;
        if (output === undefined)
            return {};
        const body = output.body ? await readJson(response) : {};
        const decoded = fromWire(output.schema, body ?? {});
        readHeaders(response.headers, output.headers, decoded);
        return decoded;
    }
    async #decodeError(operation, response) {
        const body = ((await readJson(response).catch(() => undefined)) ?? {});
        const metadata = {
            status: response.status,
            requestId: response.headers.get("x-request-id") ?? stringField(body, "requestId"),
            headers: response.headers,
        };
        const name = errorName(body, operation, this.#errors, response.status);
        const spec = name === undefined ? undefined : this.#errors[name];
        if (spec === undefined) {
            const message = stringField(body, "message") ?? `${operation.name} failed with HTTP ${response.status}`;
            return new ServiceError(name ?? "UnknownError", message, metadata, response.status >= 500 ? "server" : "client");
        }
        const fields = fromWire(spec.schema, body);
        readHeaders(response.headers, spec.headers, fields);
        return spec.create(fields, metadata);
    }
}
/**
 * Which modelled error a response is: the body's `__type` names it. When it
 * does not, the status decides, but only if one error of the operation has it.
 */
function errorName(body, operation, errors, status) {
    const named = stringField(body, "__type");
    if (named !== undefined && named in errors)
        return named;
    const byStatus = operation.errors.filter(name => errors[name]?.status === status);
    return byStatus.length === 1 ? byStatus[0] : undefined;
}
function withIdempotencyToken(operation, input) {
    const member = operation.idempotencyToken;
    const record = input;
    if (member === undefined || record[member] !== undefined)
        return input;
    // Written into the caller's own object when it can be, so resending that
    // object after a failure replays the request instead of repeating it.
    const token = globalThis.crypto.randomUUID();
    if (Object.isExtensible(input)) {
        record[member] = token;
        return input;
    }
    return { ...record, [member]: token };
}
function scalar(value, schema) {
    if (value instanceof Date) {
        const format = schema?.kind === "timestamp" ? schema.format : "date-time";
        return String(formatTimestamp(value, format));
    }
    return String(value);
}
function headerValue(value, schema) {
    if (!Array.isArray(value))
        return scalar(value, schema);
    const member = schema?.kind === "list" ? schema.member : undefined;
    return value
        .map(item => {
        const text = scalar(item, member);
        return /[",]/.test(text) ? `"${text.replace(/["\\]/g, "\\$&")}"` : text;
    })
        .join(", ");
}
function readHeaders(headers, bindings, into) {
    for (const [member, header, schema] of bindings ?? []) {
        const text = headers.get(header);
        if (text !== null)
            into[member] = parseHeader(text, schema);
    }
}
function parseHeader(text, schema) {
    switch (schema?.kind) {
        case "number":
            return Number(text);
        case "boolean":
            return text.trim() === "true";
        case "timestamp":
            return parseTimestamp(schema.format === "epoch-seconds" ? Number(text) : text);
        case "list":
            return splitHeader(text).map(item => parseHeader(item, schema.member));
        default:
            return text;
    }
}
function splitHeader(text) {
    const items = [];
    const pattern = /\s*(?:"((?:[^"\\]|\\.)*)"|([^,]*?))\s*(?:,|$)/g;
    for (const match of text.matchAll(pattern)) {
        if (match.index === text.length)
            break;
        items.push(match[1] !== undefined ? match[1].replace(/\\(.)/g, "$1") : match[2]);
    }
    return items;
}
async function readJson(response) {
    const text = await response.text();
    return text.length === 0 ? undefined : JSON.parse(text);
}
function stringField(body, name) {
    const value = body[name];
    return typeof value === "string" ? value : undefined;
}
function bearer(token) {
    return async (request, next) => {
        const value = typeof token === "function" ? await token() : token;
        if (value)
            request.headers.set("authorization", `Bearer ${value}`);
        return next(request);
    };
}
/**
 * Adds `x-amz-content-sha256` to a request with a body: a front door that
 * signs requests to its origin needs the body's digest from the caller.
 */
export const contentSha256 = async (request, next) => {
    if (request.body === null || request.headers.has("x-amz-content-sha256"))
        return next(request);
    const bytes = await request.clone().arrayBuffer();
    const digest = new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", bytes));
    request.headers.set("x-amz-content-sha256", Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join(""));
    return next(request);
};
function isAbort(error, signal) {
    return signal?.aborted === true || (error instanceof Error && error.name === "AbortError");
}
/** Full jitter: a uniform delay up to an exponentially growing cap. */
function backoff(attempt, retry) {
    const base = retry.baseDelayMs ?? 100;
    const max = retry.maxDelayMs ?? 20_000;
    return Math.random() * Math.min(max, base * 2 ** (attempt - 1));
}
function retryAfter(response) {
    const seconds = Number(response.headers.get("retry-after"));
    return Number.isFinite(seconds) && seconds > 0 ? seconds * 1000 : undefined;
}
function sleep(ms, signal) {
    return new Promise((resolve, reject) => {
        if (signal?.aborted)
            return reject(signal.reason);
        const timer = setTimeout(done, ms);
        function done() {
            signal?.removeEventListener("abort", abort);
            resolve();
        }
        function abort() {
            clearTimeout(timer);
            reject(signal.reason);
        }
        signal?.addEventListener("abort", abort, { once: true });
    });
}
/**
 * Pages of a paginated operation, following its output token until a page
 * comes back without one. A service that returns the token it was sent would
 * loop forever, so that is an error.
 */
export async function* paginate(call, input, inputToken, outputToken) {
    let token = input[inputToken];
    for (;;) {
        const page = await call({ ...input, [inputToken]: token });
        let next = page;
        for (const step of outputToken)
            next = next?.[step];
        const last = next === undefined || next === null || next === "";
        if (!last && next === token)
            throw new Error("pagination stalled: the service returned the token it was sent");
        yield page;
        if (last)
            return;
        token = next;
    }
}
