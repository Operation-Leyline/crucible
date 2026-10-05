import assert from "node:assert/strict";
import { test } from "node:test";
import { WeatherClient, TimeoutError } from "./generated/src/index.js";
import type { Location } from "./generated/src/models.js";
import { fromWire, parseJson, stringifyJson, toWire, union, string, Transport } from "./generated/src/runtime.js";
import type { UnknownUnion, OperationSpec } from "./generated/src/runtime.js";

const operation: OperationSpec = { name: 'Probe', method: 'GET', path: [], errors: [], retrySafe: true, bearer: true };
const pause = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));
function rejects(run: () => unknown) { let rejected = false; try { run(); } catch { rejected = true; } assert.ok(rejected); }

test('unknown unions expose a typed raw arm and roundtrip exact nested future values', () => {
  const schema = union(() => ({ known: string }));
  for (const payload of ['null', '{"count":9223372036854775807,"time":1790812800.123456789,"nested":[false,{"__proto__":2}]}']) {
    const text = `{"future":${payload}}`;
    const value = fromWire(schema, parseJson(text)) as UnknownUnion;
    assert.equal(value.$unknown.name, 'future');
    assert.equal(value.$unknown.json, payload);
    assert.equal(stringifyJson(toWire(schema, structuredClone(value))), text);
  }
  rejects(() => toWire(schema, { $unknown: { name: 'known', json: 'null' } }));
  rejects(() => toWire(schema, { $unknown: { name: 'future', json: 'not json' } }));
  rejects(() => toWire(schema, { future: 1 }));
  const generated: Location = { $unknown: { name: 'future', json: 'null' } };
  assert.equal(generated.$unknown.name, 'future');
});

test('total timeout includes a stalled credential provider and prevents late dispatch', async () => {
  let release!: (token: string) => void; let calls = 0;
  const provider = new Promise<string>(resolve => { release = resolve; });
  const client = new WeatherClient({ endpoint: 'https://example.test', timeoutMs: 10, token: () => provider, fetch: async () => { calls++; return new Response('{}'); } });
  let failure: unknown;
  try { await client.ping(); } catch (error) { failure = error; }
  assert.ok(failure instanceof TimeoutError);
  release('late-token'); await pause(5);
  assert.equal(calls, 0);
});

test('one timeout spans retry-after waits and ignores per-attempt restart', async () => {
  let calls = 0;
  const client = new WeatherClient({ timeoutMs: 15, fetch: async () => { calls++; return new Response('{}', { status: 503, headers: { 'retry-after': '100' } }); } });
  await assert.rejects(client.getCity({ cityId: 'a' }), /call deadline exceeded/);
  assert.equal(calls, 1);
});

test('per-call timeout overrides and null disables client budget', async () => {
  const client = new WeatherClient({ timeoutMs: 0, fetch: async () => { await pause(5); return new Response('{}'); } });
  await assert.rejects(client.ping(), /call deadline exceeded/);
  await client.ping({ timeoutMs: 1000 });
  await client.ping({ timeoutMs: null });
  await assert.rejects(client.ping({ timeoutMs: -1 }), /timeoutMs/);
});

test('cancellation and deadlines stop hung middleware, custom fetch, and response bodies', async () => {
  for (const config of [
    { middleware: [async () => new Promise<Response>(() => {})] },
    { fetch: async () => new Promise<Response>(() => {}) },
    { fetch: async () => new Response(new ReadableStream<Uint8Array>({ start() {} })) },
  ]) {
    const transport = new Transport({ endpoint: 'https://example.test', timeoutMs: 10, ...config }, { middleware: [], bearer: false }, {});
    await assert.rejects(transport.send({ ...operation, output: { body: true } }, {}), /call deadline exceeded/);
  }
  const controller = new AbortController();
  const client = new WeatherClient({ token: () => new Promise<string>(() => {}) });
  const call = client.ping({ signal: controller.signal });
  controller.abort(new Error('caller stopped'));
  await assert.rejects(call, /caller stopped/);
});


test('late middleware cannot dispatch a replacement request after the deadline', async () => {
  let release!: () => void; let calls = 0;
  const gate = new Promise<void>(resolve => { release = resolve; });
  const client = new WeatherClient({ timeoutMs: 5, middleware: [async (request, next) => {
    await gate; return next(new Request(request.url));
  }], fetch: async () => { calls++; return new Response('{}'); } });
  await assert.rejects(client.ping(), /call deadline exceeded/);
  release(); await pause(5); assert.equal(calls, 0);
});

test('timeout cancels a stalled response reader', async () => {
  let cancelled = false;
  const client = new WeatherClient({ timeoutMs: 10, fetch: async () => new Response(new ReadableStream<Uint8Array>({
    cancel() { cancelled = true; },
  })) });
  await assert.rejects(client.getCity({ cityId: 'a' }), /call deadline exceeded/);
  await pause(1); assert.ok(cancelled);
});
