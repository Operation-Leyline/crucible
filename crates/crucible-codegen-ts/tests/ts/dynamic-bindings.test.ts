import assert from "node:assert/strict";
import { test } from "node:test";
import { DynamicClient, Problem } from "./generated-dynamic/src/index.js";
import { fromWire, parseJson, stringifyJson, toWire, structure, string, union } from "./generated-dynamic/src/runtime.js";

test('dynamic query maps, greedy paths, prefix headers, json names and response codes', async () => {
  const client = new DynamicClient({ fetch: async request => {
    const url = new URL(request.url);
    assert.equal(url.pathname, '/files/a/b%20%3F%C3%A9');
    assert.equal(url.searchParams.get('fixed'), 'literal');
    assert.equal(url.searchParams.get('q'), 'explicit');
    assert.deepEqual(url.searchParams.getAll('tags'), ['first', 'a b']);
    assert.equal(request.headers.get('x-meta-custom'), 'value');
    assert.equal(await request.text(), '{"wire-value":"native"}');
    return new Response('{"wire-value":"reply"}', { status: 201, headers: { 'X-Meta-Key': 'out' } });
  } });
  const result = await client.get({ key: 'a/b ?é', explicit: 'explicit', query: { q: ['ignored'], fixed: ['ignored'], tags: ['first', 'a b'] }, metadata: { custom: 'value' }, value: 'native' });
  assert.equal(result.status, 201); assert.equal(result.value, 'reply'); assert.deepEqual(result.metadata, { key: 'out' });
});

test('explicit query/header bindings claim names even when absent and dot segments reject', async () => {
  let calls = 0;
  const client = new DynamicClient({ fetch: async request => {
    calls++;
    assert.equal(new URL(request.url).searchParams.has('q'), false);
    return new Response('{}');
  } });
  await client.get({ key: 'a/b', query: { q: ['hidden'] } });
  for (const key of ['.', '..', 'a/../b', 'a/./b']) await assert.rejects(client.get({ key }), /dot-segment/);
  assert.equal(calls, 1);
  const headers = new DynamicClient({ fetch: async request => {
    assert.equal(request.headers.get('x-explicit'), 'declared');
    assert.equal(request.headers.get('x-other'), 'dynamic');
    return new Response(null, { status: 206, headers: { 'x-explicit': 'reply' } });
  } });
  const result = await headers.headers({ values: { 'X-Explicit': 'ignored', 'x-other': 'dynamic' }, explicit: 'declared' });
  assert.deepEqual(result, { explicit: 'reply', status: 206 });
});

test('modeled errors decode prefix headers and actual HTTP response code', async () => {
  const client = new DynamicClient({ fetch: async () => new Response('{"__type":"Problem"}', { status: 418, headers: { 'X-Meta-Detail': 'value' } }) });
  let error: unknown; try { await client.get({ key: 'a' }); } catch (caught) { error = caught; }
  assert.ok(error instanceof Problem);
  assert.equal(error.status, 418); assert.deepEqual(error.details, { detail: 'value' });
});

test('paginators follow nested tokens through renamed JSON members', async () => {
  const tokens: (string | null)[] = [];
  const client = new DynamicClient({ fetch: async request => {
    tokens.push(new URL(request.url).searchParams.get('token'));
    return new Response(tokens.length === 1 ? '{"wire-next":{"wire-token":"two"}}' : '{"wire-next":{}}');
  } });
  const pages = [];
  for await (const page of client.paginatePages()) pages.push(page);
  assert.equal(pages.length, 2); assert.deepEqual(tokens, [null, 'two']);
  assert.equal(pages[0].next?.token, 'two');
});

test('renamed structures and unions distinguish wire keys from modeled properties', () => {
  const schema = structure(() => ({ a: string, b: string }), {}, [], { a: 'b', b: 'c' });
  assert.deepEqual(fromWire(schema, parseJson('{"a":"stale","b":"A","c":"B"}')), { a: 'A', b: 'B' });
  assert.equal(stringifyJson(toWire(schema, { a: 'A', b: 'B' })), '{"b":"A","c":"B"}');
  const choice = union(() => ({ native: string }), { native: 'wire' });
  assert.deepEqual(fromWire(choice, parseJson('{"wire":"value"}')), { native: 'value' });
  assert.equal(stringifyJson(toWire(choice, { native: 'value' })), '{"wire":"value"}');
  assert.deepEqual(fromWire(choice, parseJson('{"native":1}')), { $unknown: { name: 'native', json: '1' } });
});
