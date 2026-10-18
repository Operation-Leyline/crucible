import assert from "node:assert/strict";
import { test } from "node:test";
import { ServiceError, WeatherClient } from "./generated/src/index.js";
import {
  Transport, document, fromWire, integer, list, map, parseJson,
  stringifyJson, structure, toWire, union, contentSha256,
} from "./generated/src/runtime.js";
import { S_CitySummary, S_CreateCityInput } from "./generated/src/schemas.js";

function rejects(run: () => unknown): void {
  let error: unknown;
  try { run(); } catch (caught) { error = caught; }
  assert.ok(error instanceof TypeError || error instanceof SyntaxError, "expected codec rejection");
}
const long = integer(64);
const decode = (wire: string): unknown => fromWire(long, parseJson(wire));

test("long boundaries, decimal integer spellings, and exact defaults", () => {
  for (const [wire, expected] of [
    ["9223372036854775807", 9223372036854775807n],
    ["-9223372036854775808", -9223372036854775808n],
    ["9223372036854775807e0", 9223372036854775807n],
    ["1e3", 1000n], ["1.5e1", 15n], ["-0", 0n], ["0e999999", 0n],
  ] as const) assert.equal(decode(wire), expected);
  for (const wire of ["9223372036854775808", "-9223372036854775809", "9223372036854775808e0", "1.5", "1.000000000000000001", "1e999999", "1e-999999"]) rejects(() => decode(wire));
  for (const bits of [8, 16, 32] as const) {
    const schema = integer(bits);
    const bound = 2 ** (bits - 1);
    assert.equal(fromWire(schema, parseJson(String(bound - 1))), bound - 1);
    assert.equal(fromWire(schema, parseJson(String(-bound))), -bound);
    rejects(() => fromWire(schema, parseJson(String(bound))));
    rejects(() => fromWire(schema, parseJson(String(-bound - 1))));
  }
  const schema = structure(() => ({ count: long }), { count: parseJson("9223372036854775807") }, ["count"]);
  assert.deepEqual(fromWire(schema, parseJson("{}")), { count: 9223372036854775807n });
  rejects(() => fromWire(schema, parseJson('{"count":null}')));
  rejects(() => toWire(long, 1));
  rejects(() => toWire(long, 9223372036854775808n));
});

test("native bigint round trips through nested containers and known unions", () => {
  const schema = structure(() => ({ values: list(map(long)), choice: union(() => ({ count: long })) }));
  const value = { values: [{ a: 9223372036854775807n, b: -9223372036854775808n }], choice: { count: 9007199254740993n } };
  const wire = stringifyJson(toWire(schema, value));
  assert.equal(wire, '{"values":[{"a":9223372036854775807,"b":-9223372036854775808}],"choice":{"count":9007199254740993}}');
  assert.deepEqual(fromWire(schema, parseJson(wire)), value);
  rejects(() => fromWire(schema, parseJson('{"values":[{"a":9223372036854775808}]}')));
  const choice = union(() => ({ count: long }));
  rejects(() => fromWire(choice, parseJson('{}')));
  rejects(() => fromWire(choice, parseJson('{"count":1,"future":2}')));
  rejects(() => fromWire(choice, parseJson('{"count":"wrong"}')));
});

test("IR nullability permits client optional inputs and required document null", () => {
  assert.deepEqual(fromWire(S_CreateCityInput, parseJson('{"name":"n","clientToken":null}')), { name: "n", clientToken: null });
  const schema = structure(() => ({ value: document }), {}, ["value"]);
  assert.deepEqual(fromWire(schema, parseJson('{"value":null}')), { value: null });
  rejects(() => fromWire(schema, parseJson('{}')));
  rejects(() => toWire(S_CitySummary, { id: "c", name: "n", population: null }));
});

test("parser keys stay own data, grammar is strict, and documents reject nonfinite values", () => {
  const text = '{"__proto__":{"polluted":true},"constructor":1,"prototype":2,"value":9007199254740993}';
  const value = fromWire(document, parseJson(text)) as Record<string, unknown>;
  assert.equal(Object.getPrototypeOf(value), Object.prototype);
  assert.ok(Object.hasOwn(value, "__proto__"));
  assert.equal(({} as Record<string, unknown>).polluted, undefined);
  assert.equal(value.value, 9007199254740993n);
  assert.ok(stringifyJson(value).includes('"__proto__"'));
  for (const text of ["01", "1.", "[1,]", "true false", '"\\x00"']) rejects(() => parseJson(text));
  rejects(() => parseJson("[".repeat(257) + "0" + "]".repeat(257)));
  rejects(() => fromWire(document, parseJson('{"nested":[1e999]}')));
  rejects(() => stringifyJson({ nested: [Infinity] }));
  rejects(() => stringifyJson(NaN));
  rejects(() => stringifyJson(new Date()));
  rejects(() => stringifyJson(new Map()));
  assert.equal(stringifyJson(Array(2)), "[null,null]");
});

test("real response path validates raw longs before rounding without retry", async () => {
  for (const wire of ["9223372036854775807", "-9223372036854775808", "9223372036854775808", "1.000000000000000001"]) {
    let attempts = 0;
    const client = new WeatherClient({ fetch: async () => {
      attempts++;
      return new Response(`{"city":{"id":"c","name":"n","population":${wire}}}`);
    } });
    if (wire === "9223372036854775807" || wire === "-9223372036854775808") {
      assert.equal((await client.getCity({ cityId: "c" })).city.population, BigInt(wire));
    } else await assert.rejects(client.getCity({ cityId: "c" }), /integer/);
    assert.equal(attempts, 1);
  }
});

test("a write without a body still carries the empty body's digest; a bodiless read carries none", async () => {
  const seen: Request[] = [];
  const transport = new Transport({ endpoint: "https://example.test", fetch: async request => {
    seen.push(request);
    return new Response("{}");
  } }, { bearer: false, middleware: [contentSha256] }, {});
  const empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
  for (const method of ["POST", "PUT", "PATCH", "GET", "DELETE"]) {
    const operation = { name: "Bare", method, path: [], errors: [], retrySafe: false,
      input: structure(() => ({}), {}, []), query: [], headers: [],
      output: { body: true, schema: structure(() => ({}), {}, []), headers: [] } };
    await transport.send(operation, {});
  }
  assert.deepEqual(seen.map(request => request.headers.get("x-amz-content-sha256")),
    [empty, empty, empty, null, null]);
});

test("bound longs, exact request body digest and pre-dispatch overflow", async () => {
  let attempts = 0;
  const schema = structure(() => ({ count: long }), {}, ["count"]);
  const transport = new Transport({ endpoint: "https://example.test", fetch: async request => {
    attempts++;
    const body = await request.text();
    assert.equal(body, '{"count":9223372036854775807}');
    assert.equal(new URL(request.url).searchParams.get("count"), "9223372036854775807");
    assert.equal(request.headers.get("x-count"), "9223372036854775807");
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(body)));
    assert.equal(request.headers.get("x-amz-content-sha256"), Array.from(digest, b => b.toString(16).padStart(2, "0")).join(""));
    return new Response('{}', { headers: { "x-count": "9223372036854775807" } });
  } }, { bearer: false, middleware: [contentSha256] }, {});
  const operation = { name: "Exact", method: "POST", path: [], errors: [], retrySafe: false,
    input: schema, body: ["count"], query: [["count", "count", long] as const], headers: [["count", "x-count", long] as const],
    output: { body: true, schema, headers: [["count", "x-count", long] as const] } };
  assert.deepEqual(await transport.send(operation, { count: 9223372036854775807n }), { count: 9223372036854775807n });
  await assert.rejects(transport.send(operation, { count: 9223372036854775808n }), /range/);
  assert.equal(attempts, 1);
});

test("malformed modeled error fields retain identity metadata and throttle retries", async () => {
  let attempts = 0;
  const transport = new Transport({ endpoint: "https://example.test", retry: { maxAttempts: 2, baseDelayMs: 0 }, fetch: async () => {
    attempts++;
    return new Response('{"__type":"Throttle","count":9223372036854775808}', { status: 429, headers: { "x-request-id": "retained" } });
  } }, { bearer: false, middleware: [] }, {
    Throttle: { status: 429, fault: "client", retryable: { throttling: true }, schema: structure(() => ({ count: long })),
      create: () => { throw new Error("Malformed fields must not construct a modeled error"); } },
  });
  const error = await transport.send({ name: "Unsafe", method: "POST", path: [], retrySafe: false, errors: ["Throttle"] }, {}).catch(error => error);
  assert.ok(error instanceof ServiceError);
  assert.equal(error.name, "Throttle");
  assert.equal(error.$metadata.status, 429);
  assert.equal(error.$metadata.requestId, "retained");
  assert.equal(error.$retryable?.throttling, true);
  assert.equal(attempts, 2);
});

test("malformed bound integer errors retain HTTP metadata and retry traits", async () => {
  let attempts = 0;
  const client = new WeatherClient({ retry: { maxAttempts: 2, baseDelayMs: 0 }, fetch: async () => {
    attempts++;
    return new Response('{"__type":"ThrottlingError","message":"slow"}', { status: 429,
      headers: { "retry-after": "0.001", "x-request-id": "header-retained" } });
  } });
  const error = await client.ping().catch(error => error);
  assert.ok(error instanceof ServiceError);
  assert.equal(error.name, "ThrottlingError");
  assert.equal(error.$metadata.requestId, "header-retained");
  assert.equal(error.$retryable?.throttling, true);
  assert.equal(attempts, 2);
});

test("unknown modeled error properties cannot overwrite HTTP metadata or prototype", async () => {
  const client = new WeatherClient({ retry: false, fetch: async () => new Response(
    '{"__type":"ConflictError","message":"taken","$metadata":{"status":200},"__proto__":{"polluted":true}}',
    { status: 409, headers: { "x-request-id": "real" } },
  ) });
  const error = await client.createCity({ name: "c" }).catch(error => error);
  assert.ok(error instanceof ServiceError);
  assert.equal(error.name, "ConflictError");
  assert.equal(error.$metadata.status, 409);
  assert.equal(error.$metadata.requestId, "real");
  assert.equal((error as unknown as Record<string, unknown>).polluted, undefined);
});
