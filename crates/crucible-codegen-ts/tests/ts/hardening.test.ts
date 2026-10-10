import assert from "node:assert/strict";
import { test } from "node:test";
import { WeatherClient } from "./generated/src/index.js";
import { HardeningClient } from "./generated-hardening/src/index.js";
import { Transport, document, fromWire, integer, list, map, parseJson, string, toWire } from "./generated-hardening/src/runtime.js";
import { S_Collections, S_DefaultInput, S_Defaults, S_TimestampDefaults } from "./generated-hardening/src/schemas.js";
import type { OperationSpec, Schema } from "./generated-hardening/src/runtime.js";

const operation: OperationSpec = { name: "Probe", method: "GET", path: ["probe"], errors: [], retrySafe: true };
const success = () => new Response('{}', { status: 200 });
function rejects(run: () => unknown): void {
  let error: unknown;
  try { run(); } catch (caught) { error = caught; }
  assert.ok(error instanceof TypeError, "expected a codec error");
}

test("generated operation and paginator names remain callable without collisions", async () => {
  const paths: string[] = [];
  const client = new HardeningClient({ fetch: async request => { paths.push(new URL(request.url).pathname); return success(); } });
  await client.constructor_3();
  await client.constructor_2();
  await client.get();
  await client.get_2();
  await client.paginateListThings();
  for await (const _ of client.paginateListThings_2()) { /* Consume the single page. */ }
  assert.deepEqual(paths, ['/Constructor', '/Constructor_2', '/GET', '/Get', '/PaginateListThings', '/ListThings']);
});

test("collection-only scalar schemas compile and preserve their values", () => {
  const value = { texts: ['x'], flags: [true], numbers: [1.5], documents: [{ nil: null }, { a: 1 }], textsMap: { a: 'x' }, flagsMap: { a: false }, numbersMap: { a: 2.5 }, documentsMap: { a: { nil: null } } };
  assert.deepEqual(fromWire(S_Collections, toWire(S_Collections, value)), value);
});

test("clientOptional and input defaults stay absent while ordinary defaults apply", () => {
  assert.deepEqual(fromWire(S_Defaults, parseJson('{}')), { ordinary: 7 });
  assert.deepEqual(fromWire(S_DefaultInput, parseJson('{}')), {});
  assert.deepEqual(fromWire(S_Defaults, parseJson('{"optional":null}')), { ordinary: 7, optional: null });
});

test("dense list and map values reject null and missing entries, including document elements", () => {
  for (const scalar of [string, document, integer(32), list(string), map(string)]) {
    for (const convert of [toWire, fromWire]) {
      for (const value of [null, undefined]) {
        rejects(() => convert(list(scalar), [value]));
        rejects(() => convert(map(scalar), { a: value }));
      }
      rejects(() => convert(list(scalar), Array(1)));
    }
  }
  for (const convert of [toWire, fromWire]) {
    assert.deepEqual(convert(list(document), [{ nil: null }]), [{ nil: null }]);
    assert.deepEqual(convert(map(document), { a: { nil: null } }), { a: { nil: null } });
    rejects(() => convert(list(document), [undefined]));
  }
});

test("timestamp defaults decode from model values independently of wire format", () => {
  assert.deepEqual(fromWire(S_TimestampDefaults, parseJson('{}')), {
    numericDateTime: new Date('1969-12-31T23:59:59.750Z'),
    stringEpoch: new Date('2026-10-01T00:00:00.125Z'),
    numericHttpDate: new Date('2026-10-01T00:00:00Z'),
  });
});

test("null in a dense request fails before dispatch and malformed success never retries", async () => {
  const schema: Schema = list(string);
  let calls = 0;
  const transport = new Transport({ endpoint: 'https://example.test', fetch: async () => { calls++; return new Response('{"values":[null]}'); } }, { middleware: [], bearer: false }, {});
  const { structure } = await import('./generated-hardening/src/runtime.js');
  const input = structure(() => ({ values: schema }));
  await assert.rejects(transport.send({ ...operation, input, body: ['values'], method: 'POST' }, { values: [null] }), /dense collection/);
  assert.equal(calls, 0);
  await assert.rejects(transport.send({ ...operation, output: { body: true, schema: input } }, {}), /dense collection/);
  assert.equal(calls, 1);
});

test("retries snapshot nested mutable input while preserving token write-back", async () => {
  const input = { name: 'original', location: { coordinates: { latitude: 1, longitude: 2 } } };
  const bodies: string[] = [];
  const client = new WeatherClient({ retry: { maxAttempts: 2, baseDelayMs: 0 }, fetch: async request => {
    bodies.push(await request.text());
    input.name = 'changed'; input.location.coordinates.latitude = 99;
    return new Response(bodies.length === 1 ? '{}' : '{"city":{"id":"c","name":"original"}}', { status: bodies.length === 1 ? 503 : 200 });
  } });
  await client.createCity(input);
  assert.equal(bodies.length, 2);
  assert.equal(bodies[0], bodies[1]);
  const wire = JSON.parse(bodies[0]);
  assert.equal(wire.name, 'original');
  assert.equal(wire.location.coordinates.latitude, 1);
  assert.equal(wire.clientToken, (input as typeof input & { clientToken: string }).clientToken);
});

test("dot-segment labels reject before hooks or dispatch while ordinary dots survive", async () => {
  let calls = 0; let providers = 0; let url = '';
  const transport = new Transport({ endpoint: 'https://example.test/base', token: () => { providers++; return 'token'; }, fetch: async request => { calls++; url = request.url; return success(); } }, { middleware: [], bearer: true }, {});
  const label = { ...operation, path: ['objects', { label: 'id' }] };
  for (const id of ['.', '..']) await assert.rejects(transport.send(label, { id }), /dot-segment/);
  assert.equal(calls, 0); assert.equal(providers, 0);
  await transport.send(label, { id: 'a..b' });
  assert.equal(url, 'https://example.test/base/objects/a..b');
});

test("effective operation auth skips the provider on public calls including retries", async () => {
  let providers = 0; let publicAttempts = 0;
  const seen: (string | null)[] = [];
  const client = new HardeningClient({ token: () => `token-${++providers}`, retry: { baseDelayMs: 0 }, middleware: [async (request, next) => { seen.push(request.headers.get('authorization')); return next(request); }], fetch: async request => new Response('{}', { status: new URL(request.url).pathname === '/Public' && ++publicAttempts === 1 ? 503 : 200 }) });
  await client.public();
  assert.equal(providers, 0);
  await client.protected();
  assert.equal(providers, 1);
  assert.deepEqual(seen, [null, null, 'Bearer token-1']);
  const transport = new Transport({ endpoint: 'https://example.test', token: () => `token-${++providers}`, fetch: async request => { assert.equal(request.headers.get('authorization'), 'Bearer token-2'); return success(); } }, { middleware: [], bearer: false }, {});
  await transport.send({ ...operation, bearer: true }, {});
});

test("Retry-After accepts HTTP dates and zero without falling back to jitter", async () => {
  const setTimer = globalThis.setTimeout; const now = Date.now; const random = Math.random;
  const delays: (number | undefined)[] = [];
  Date.now = () => Date.UTC(2026, 9, 4); Math.random = () => .5;
  globalThis.setTimeout = (handler, delay, ...args) => { delays.push(delay); return setTimer(handler, 0, ...args); };
  try {
    for (const header of [new Date(Date.now() + 5000).toUTCString(), '0', '-1', 'invalid']) {
      let calls = 0;
      const transport = new Transport({ endpoint: 'https://example.test', retry: { maxAttempts: 2, baseDelayMs: 100 }, fetch: async () => new Response('{}', { status: ++calls === 1 ? 503 : 200, headers: { 'retry-after': header } }) }, { middleware: [], bearer: false }, {});
      await transport.send(operation, {});
    }
    assert.deepEqual(delays, [5000, 0, 50, 50]);
  } finally { globalThis.setTimeout = setTimer; Date.now = now; Math.random = random; }
});

test("oversized Retry-After stays abortable and never overflows a native timer", async () => {
  const setTimer = globalThis.setTimeout;
  for (const header of ['2147484', '1e300', '1e999']) {
    const controller = new AbortController(); const reason = new Error('cancel large delay');
    const delays: (number | undefined)[] = []; let calls = 0;
    globalThis.setTimeout = (handler, delay, ...args) => { delays.push(delay); queueMicrotask(() => controller.abort(reason)); return setTimer(handler, 1000, ...args); };
    try {
      const transport = new Transport({ endpoint: 'https://example.test', fetch: async () => { calls++; return new Response('{}', { status: 503, headers: { 'retry-after': header } }); } }, { middleware: [], bearer: false }, {});
      await assert.rejects(transport.send(operation, {}, { signal: controller.signal }), /cancel large delay/);
      assert.equal(calls, 1); assert.deepEqual(delays, [2147483647]);
    } finally { globalThis.setTimeout = setTimer; }
  }
});

test("a client names the release it was generated for in Crucible-Version", async () => {
  const sent: Request[] = [];
  const transport = new Transport({ endpoint: 'https://example.test', headers: { 'Crucible-Version': 'configured' }, fetch: async request => { sent.push(request); return success(); } }, { middleware: [], bearer: false }, {});
  await transport.send(operation, {});
  assert.equal(sent[0].headers.get('crucible-version'), 'configured');
  // The generated spec's release wins over a configured header: the client's types are that release's.
  await transport.send({ ...operation, apiVersion: '2026-09-04' }, {});
  assert.equal(sent[1].headers.get('crucible-version'), '2026-09-04');
});
