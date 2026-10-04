// The generated TypeScript client against the generated Rust server, over
// real HTTP. tools/test-e2e.sh builds both from the weather fixture, starts
// the server and sets WEATHER_URL.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  ConflictError,
  NotFoundError,
  ThrottlingError,
  ValidationError,
  WeatherClient,
} from "../generated/weather-ts/src/index.js";

const endpoint = process.env.WEATHER_URL;
if (!endpoint) throw new Error("WEATHER_URL is not set: run tools/test-e2e.sh");

const client = new WeatherClient({ endpoint, retry: { maxAttempts: 3, baseDelayMs: 1, maxDelayMs: 5 } });
const unique = (prefix: string) => `${prefix}-${Math.random().toString(36).slice(2, 10)}`;

test("a created city reads back with every member intact", async () => {
  const name = unique("oslo");
  const created = await client.createCity({
    name,
    expectedRevision: 7,
    location: { coordinates: { latitude: 59.9, longitude: 10.7 } },
  });
  assert.match(created.city.id, /^c-\d{8}$/);
  assert.equal(created.city.population, 7, "a header member reached the server");
  assert.ok(created.city.updatedAt instanceof Date, "date-time body timestamp");
  assert.ok(created.city.createdAt instanceof Date, "epoch-seconds body timestamp");
  assert.equal(created.city.createdAt!.getTime(), created.city.updatedAt!.getTime());

  const at = new Date("2026-10-01T12:30:00Z");
  const read = await client.getCity({ cityId: created.city.id, at });
  assert.equal(read.city.name, name);
  assert.equal(read.requestId, "req-from-server", "a response header member");
  assert.deepEqual(read.forecast, { clear: { at: "2026-10-01T12:30:00+00:00" } }, "a query timestamp, a map, a document");
});

test("resending the same input replays instead of creating twice", async () => {
  const input = { name: unique("bergen") };
  const first = await client.createCity(input);
  const second = await client.createCity(input);
  assert.equal(second.city.id, first.city.id);
});

test("a modelled error arrives as its class", async () => {
  const name = unique("tromso");
  await client.createCity({ name });
  const conflict = await client.createCity({ name }).catch(e => e);
  assert.ok(conflict instanceof ConflictError);
  assert.equal(conflict.$metadata.status, 409);
  assert.match(conflict.message, /exists/);

  const missing = await client.getCity({ cityId: "c-99999999" }).catch(e => e);
  assert.ok(missing instanceof NotFoundError);
  assert.equal(missing.resourceId, "c-99999999");
});

test("a broken constraint is a validation error naming the member", async () => {
  const error = await client.createCity({ name: "x".repeat(41) }).catch(e => e);
  assert.ok(error instanceof ValidationError, String(error));
  assert.equal(error.$metadata.status, 400);
  assert.deepEqual(error.fieldList, [{ path: "name", message: "length must be at most 40" }]);
});

test("labels and query parameters are validated too", async () => {
  const badLabel = await client.getCity({ cityId: "nope" }).catch(e => e);
  assert.ok(badLabel instanceof ValidationError);
  assert.equal(badLabel.fieldList?.[0]?.path, "cityId");

  const badRange = await client.listCities({ maxResults: 0 }).catch(e => e);
  assert.ok(badRange instanceof ValidationError);
  assert.equal(badRange.fieldList?.[0]?.path, "maxResults");
});

test("a union with two members set is refused", async () => {
  const location = { coordinates: { latitude: 1, longitude: 2 }, unknown: {} } as never;
  const error = await client.createCity({ name: unique("two"), location }).catch(e => e);
  assert.ok(error instanceof ValidationError, String(error));
});

test("pagination walks every page; list query parameters repeat", async () => {
  const names = [unique("p"), unique("p"), unique("p")];
  for (const name of names) await client.createCity({ name });
  const seen: string[] = [];
  for await (const page of client.paginateListCities({ maxResults: 2 })) {
    assert.ok(page.cities.length <= 2);
    seen.push(...page.cities.map(city => city.name));
  }
  for (const name of names) assert.ok(seen.includes(name), `${name} missing`);

  const filtered = await client.listCities({ tags: [names[1]] });
  assert.deepEqual(filtered.cities.map(city => city.name), [names[1]]);
});

test("throttling is retried, then surfaces with its header member", async () => {
  const key = unique("throttle");
  await client.ping({ headers: { "x-fail-times": "2", "x-fail-key": key } });

  const stubborn = unique("throttle");
  const error = await client.ping({ headers: { "x-fail-times": "9", "x-fail-key": stubborn } }).catch(e => e);
  assert.ok(error instanceof ThrottlingError, String(error));
  assert.equal(error.retryAfter, 0);
});

test("an operation with empty input and output round-trips", async () => {
  assert.deepEqual(await client.status(), {});
});
