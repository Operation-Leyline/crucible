// Behaviour of a generated client, end to end against a fake fetch. The
// client under test is generated from crates/crucible-core/tests/fixtures/
// weather.json by tools/test-ts.sh.
import assert from "node:assert/strict";
import { test } from "node:test";

import { ConflictError, NetworkError, ServiceError, WeatherException, Sky, ThrottlingError, WeatherClient } from "./generated/src/index.js";
import type { Middleware } from "./generated/src/index.js";
import { Transport, blob, fromWire, string, toWire } from "./generated/src/runtime.js";
import type { OperationSpec } from "./generated/src/runtime.js";

interface Sent {
  method: string;
  url: URL;
  headers: Headers;
  body: string | undefined;
}

/** A fetch that records requests and answers from a queue. */
function fake(...responses: (Response | Error)[]) {
  const sent: Sent[] = [];
  const fetch = async (request: Request): Promise<Response> => {
    const body = request.body === null ? undefined : await request.text();
    sent.push({ method: request.method, url: new URL(request.url), headers: request.headers, body });
    const next = responses.shift();
    if (next === undefined) throw new Error("no response queued");
    if (next instanceof Error) throw next;
    return next;
  };
  return { sent, fetch };
}

const json = (status: number, body: unknown, headers: Record<string, string> = {}) =>
  new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json", ...headers } });

const quick = { maxAttempts: 3, baseDelayMs: 1, maxDelayMs: 2 };

test("successful operations without modeled bodies leave failing and stalled streams unread", async () => {
  const outputs: (OperationSpec["output"])[] = [undefined, {
    body: false, headers: [["requestId", "x-request-id", string]],
  }];
  for (const output of outputs) {
    for (const broken of [true, false]) {
      const response = new Response(new ReadableStream({
        start(controller) { if (broken) controller.error(new TypeError("broken body")); },
      }), { headers: { "x-request-id": "req-1" } });
      const transport = new Transport({ endpoint: "https://weather.example", retry: false,
        fetch: async () => response,
      }, { middleware: [], bearer: false }, {});
      const result = transport.send({ name: "NoBody", method: "GET", path: [], errors: [], retrySafe: true, output }, {});
      // Observe the body before awaiting completion so the stalled-stream
      // regression fails immediately instead of hanging the test.
      void result.catch(() => {});
      await Promise.resolve();
      assert.equal(response.bodyUsed, false);
      assert.deepEqual(await result, output === undefined ? {} : { requestId: "req-1" });
      await response.body?.cancel().catch(() => {});
    }
  }
});

test("body-read retries honor Retry-After and dispatch failures use backoff", async () => {
  const delays: (number | undefined)[] = [];
  const setTimer = globalThis.setTimeout;
  globalThis.setTimeout = (handler, delay, ...args) => {
    delays.push(delay);
    return setTimer(handler, 0, ...args);
  };
  try {
    const broken = new Response(new ReadableStream({
      start(controller) { controller.error(new TypeError("broken error body")); },
    }), { status: 503, headers: { "retry-after": "2" } });
    const { fetch, sent } = fake(broken, new TypeError("fetch failed"), json(200, {}));
    // Ping has no modeled output, but its error body must still be read.
    await new WeatherClient({ fetch, retry: { maxAttempts: 3, baseDelayMs: 0 } }).ping();
    assert.equal(sent.length, 3);
    assert.deepEqual(delays, [2000, 0], "a failed dispatch must not reuse the previous response header");
  } finally {
    globalThis.setTimeout = setTimer;
  }
});

test("a body failure never replays an unsafe operation", async () => {
  let calls = 0;
  const transport = new Transport({ endpoint: "https://weather.example", retry: quick, fetch: async () => {
    calls++;
    return new Response(new ReadableStream({ start(c) { c.error(new TypeError("disconnected")); } }));
  } }, { middleware: [], bearer: false }, {});
  const error = await transport.send({ name: "Unsafe", method: "POST", path: ["unsafe"], errors: [], retrySafe: false, output: { body: true } }, {}).catch(e => e);
  assert.ok(error instanceof NetworkError);
  assert.ok(!(error instanceof WeatherException));
  assert.equal(calls, 1);
});

test("body transport failures retry for success and error responses", async () => {
  for (const status of [200, 503]) {
    const broken = new Response(new ReadableStream({
      start(controller) { controller.error(new TypeError("connection lost mid-body")); },
    }), { status });
    const { sent, fetch } = fake(broken, json(200, { cities: [] }));
    const output = await new WeatherClient({ fetch, retry: quick }).listCities({});
    assert.deepEqual(output.cities, []);
    assert.equal(sent.length, 2);
  }
});

test("invalid input and invalid response JSON are not network failures", async () => {
  const { sent, fetch } = fake(new Response("not json", { status: 200 }));
  const client = new WeatherClient({ fetch, retry: quick });
  const invalid = await client.getCity({ cityId: "" }).catch(e => e);
  assert.ok(invalid instanceof TypeError);
  assert.equal(sent.length, 0);
  const malformed = await client.listCities({}).catch(e => e);
  assert.ok(malformed instanceof SyntaxError);
  assert.equal(sent.length, 1);
});

test("aborting an error body preserves cancellation without retry", async () => {
  let calls = 0;
  const controller = new AbortController();
  const reason = new DOMException("cancelled", "AbortError");
  const fetch = async () => {
    calls++;
    return new Response(new ReadableStream({
      pull(stream) { controller.abort(reason); stream.error(reason); },
    }), { status: 503 });
  };
  const error = await new WeatherClient({ fetch, retry: quick }).listCities({}, { signal: controller.signal }).catch(e => e);
  assert.equal(error, reason);
  assert.equal(calls, 1);
});

test("labels are encoded, queries repeat, timestamps take their location's format", async () => {
  const { sent, fetch } = fake(json(200, { city: { id: "c-1", name: "Oslo" } }, { "X-Request-Id": "r-1" }));
  const client = new WeatherClient({ fetch, token: "secret" });
  const out = await client.getCity({ cityId: "c/1 2", at: new Date("2026-10-01T12:00:00Z") });

  assert.equal(sent[0].url.href, "https://weather.example/cities/c%2F1%202?at=2026-10-01T12%3A00%3A00.000Z");
  assert.equal(sent[0].headers.get("authorization"), "Bearer secret");
  assert.equal(sent[0].body, undefined);
  assert.equal(out.requestId, "r-1");
  assert.equal(out.city.population, 0n, "a missing member with a default takes it");
});

test("list query parameters repeat their key", async () => {
  const { sent, fetch } = fake(json(200, { cities: [] }));
  await new WeatherClient({ fetch }).listCities({ tags: ["a", "b,c"], maxResults: 5 });
  assert.deepEqual(sent[0].url.searchParams.getAll("tag"), ["a", "b,c"]);
  assert.equal(sent[0].url.searchParams.get("maxResults"), "5");
});

test("bodies carry body members only, with a digest and a filled idempotency token", async () => {
  const { sent, fetch } = fake(json(201, { city: { id: "c-1", name: "Oslo", population: 7, updatedAt: "2026-09-21T14:13:20Z", createdAt: 1_790_000_000 } }));
  const input = { name: "Oslo", expectedRevision: 3n, location: { coordinates: { latitude: 1, longitude: 2 } } };
  const out = await new WeatherClient({ fetch }).createCity(input);

  const body = JSON.parse(sent[0].body!);
  assert.deepEqual(Object.keys(body).sort(), ["clientToken", "location", "name"]);
  assert.match(body.clientToken, /^[0-9a-f-]{36}$/);
  assert.equal((input as { clientToken?: string }).clientToken, body.clientToken, "written back for replay");
  assert.equal(sent[0].headers.get("x-expected-revision"), "3");
  assert.equal(sent[0].headers.get("content-type"), "application/json");

  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(sent[0].body)));
  const hex = Array.from(digest, b => b.toString(16).padStart(2, "0")).join("");
  assert.equal(sent[0].headers.get("x-amz-content-sha256"), hex);

  assert.ok(out.city.updatedAt instanceof Date);
  assert.equal(out.city.updatedAt!.toISOString(), "2026-09-21T14:13:20.000Z");
  assert.equal(out.city.createdAt!.toISOString(), "2026-09-21T14:13:20.000Z", "@timestampFormat overrides");
  assert.equal(out.city.population, 7n);
});

test("a JSON body keeps its media type over client and per-call Content-Type defaults", async () => {
  const { sent, fetch } = fake(json(201, { city: { id: "c-1", name: "Oslo" } }), json(201, { city: { id: "c-2", name: "Bergen" } }));
  const client = new WeatherClient({ fetch, headers: { "Content-Type": "text/plain" } });
  await client.createCity({ name: "Oslo", location: { coordinates: { latitude: 1, longitude: 2 } } });
  await client.createCity({ name: "Bergen", location: { coordinates: { latitude: 1, longitude: 2 } } }, { headers: { "content-type": "text/csv" } });
  assert.deepEqual(sent.map(request => request.headers.get("content-type")), ["application/json", "application/json"]);
});

test("redirects are returned, never followed, even for a request a middleware rebuilt", async () => {
  const modes: RequestRedirect[] = [];
  const rebuild: Middleware = (request, next) => next(new Request(request.url, { method: request.method, headers: request.headers }));
  for (const middleware of [[], [rebuild]]) {
    const fetch = async (request: Request): Promise<Response> => {
      modes.push(request.redirect);
      return new Response(null, { status: 302, headers: { location: "https://elsewhere.example/" } });
    };
    const error = await new WeatherClient({ fetch, token: "secret", middleware, retry: quick }).ping().catch(e => e);
    assert.ok(error instanceof ServiceError);
    assert.equal(error.$metadata.status, 302);
  }
  assert.deepEqual(modes, ["manual", "manual"], "one attempt each, with the redirect mode set");
});

test("a frozen input gets a token on a copy", async () => {
  const { sent, fetch } = fake(json(201, { city: { id: "c-1", name: "Oslo" } }));
  await new WeatherClient({ fetch }).createCity(Object.freeze({ name: "Oslo" }));
  assert.ok(JSON.parse(sent[0].body!).clientToken);
});

test("a retried request reuses its token", async () => {
  const { sent, fetch } = fake(json(503, { message: "busy" }), json(201, { city: { id: "c-1", name: "Oslo" } }));
  await new WeatherClient({ fetch, retry: quick }).createCity({ name: "Oslo" });
  assert.equal(sent.length, 2);
  assert.equal(JSON.parse(sent[0].body!).clientToken, JSON.parse(sent[1].body!).clientToken);
});

test("errors decode to their class by status when unambiguous", async () => {
  const { fetch } = fake(json(409, { message: "taken" }, { "x-request-id": "req-9" }));
  const error = await new WeatherClient({ fetch }).createCity({ name: "Oslo" }).catch(e => e);
  assert.ok(error instanceof ConflictError);
  assert.ok(error instanceof ServiceError);
  assert.ok(error instanceof WeatherException);
  assert.equal(error.message, "taken");
  assert.equal(error.$metadata.status, 409);
  assert.equal(error.$metadata.requestId, "req-9");
  assert.equal(error.$fault, "client");
});

test("the body's __type wins over the status", async () => {
  const { fetch } = fake(json(400, { __type: "ThrottlingError", message: "slow down" }));
  const error = await new WeatherClient({ fetch, retry: false }).createCity({ name: "Oslo" }).catch(e => e);
  assert.ok(error instanceof ThrottlingError);
});

test("throttling retries, honouring Retry-After, and decodes header members", async () => {
  const { sent, fetch } = fake(
    json(429, { message: "slow" }, { "retry-after": "0" }),
    json(429, { message: "slow" }, { "retry-after": "0" }),
    json(429, { message: "slow" }, { "retry-after": "0" }),
  );
  const error = await new WeatherClient({ fetch, retry: quick }).ping().catch(e => e);
  assert.equal(sent.length, 3);
  assert.ok(error instanceof ThrottlingError);
  assert.equal(error.retryAfter, 0);
});

test("an unknown error keeps status and message", async () => {
  const { fetch } = fake(json(418, { message: "teapot" }));
  const error = await new WeatherClient({ fetch }).ping().catch(e => e);
  assert.ok(error instanceof ServiceError);
  assert.ok(error instanceof WeatherException);
  assert.equal(error.name, "UnknownError");
  assert.equal(error.message, "teapot");
});

test("network failures retry when safe, then surface as NetworkError", async () => {
  const { sent, fetch } = fake(new TypeError("fetch failed"), new TypeError("fetch failed"), new TypeError("fetch failed"));
  const error = await new WeatherClient({ fetch, retry: quick }).ping().catch(e => e);
  assert.equal(sent.length, 3);
  assert.ok(error instanceof NetworkError);
  assert.ok(!(error instanceof WeatherException));
});

test("an aborted request is not retried", async () => {
  const controller = new AbortController();
  const fetch = async () => {
    controller.abort();
    throw new DOMException("aborted", "AbortError");
  };
  const error = await new WeatherClient({ fetch, retry: quick }).ping({ signal: controller.signal }).catch(e => e);
  assert.equal(error.name, "AbortError");
});

test("paginators follow the token and stop when it is absent", async () => {
  const { sent, fetch } = fake(
    json(200, { cities: [{ id: "c-1", name: "A" }], nextToken: "t1" }),
    json(200, { cities: [{ id: "c-2", name: "B" }] }),
  );
  const names: string[] = [];
  for await (const page of new WeatherClient({ fetch }).paginateListCities({ maxResults: 1 })) {
    names.push(...page.cities.map(city => city.name));
  }
  assert.deepEqual(names, ["A", "B"]);
  assert.equal(sent[1].url.searchParams.get("nextToken"), "t1");
  assert.equal(sent[1].url.searchParams.get("maxResults"), "1");
});

test("a paginator refuses a token that does not advance", async () => {
  const { fetch } = fake(json(200, { cities: [], nextToken: "same" }), json(200, { cities: [], nextToken: "same" }));
  const pages = new WeatherClient({ fetch }).paginateListCities({});
  await pages.next();
  await assert.rejects(pages.next(), /stalled/);
});

test("caller middleware runs on every attempt after the client's own", async () => {
  const seen: (string | null)[] = [];
  const home: Middleware = (request, next) => {
    seen.push(request.headers.get("authorization"));
    request.headers.set("x-home", "use1");
    return next(request);
  };
  const { sent, fetch } = fake(json(503, {}), json(200, {}));
  let calls = 0;
  const token = () => `t${++calls}`;
  await new WeatherClient({ fetch, token, middleware: [home], retry: quick }).ping();
  assert.deepEqual(seen, ["Bearer t1", "Bearer t2"], "the token provider is asked per attempt");
  assert.equal(sent[1].headers.get("x-home"), "use1");
});

test("a payload goes raw under its media type, and the digest covers exactly those bytes", async () => {
  // Not UTF-8, and JSON-looking in places: neither may be reinterpreted.
  const photo = new Uint8Array([0xff, 0xd8, 0x7b, 0x22, 0x00, 0x80, 0xfe, 0x01, 0x02]);
  const seen: { headers: Headers; bytes: Uint8Array }[] = [];
  const responses = [
    json(200, { photo: { size: 9, thumbnail: "/9h7IgCA/gE=", caption: "harbour" } }),
    new Response(photo, { headers: { "content-type": "image/jpeg", "x-photo-caption": "harbour" } }),
    new Response(null, { status: 200 }),
  ];
  const fetch = async (request: Request): Promise<Response> => {
    seen.push({ headers: request.headers, bytes: new Uint8Array(await request.arrayBuffer()) });
    return responses.shift()!;
  };
  const client = new WeatherClient({ fetch });

  const stored = await client.putCityPhoto({ cityId: "c-00000001", caption: "harbour", photo });
  assert.deepEqual(seen[0].bytes, photo);
  assert.equal(seen[0].headers.get("content-type"), "image/jpeg");
  assert.equal(seen[0].headers.get("x-photo-caption"), "harbour");
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", photo));
  const hex = Array.from(digest, b => b.toString(16).padStart(2, "0")).join("");
  assert.equal(seen[0].headers.get("x-amz-content-sha256"), hex);
  assert.ok(stored.photo.thumbnail instanceof Uint8Array, "base64 in JSON decodes to bytes");
  assert.deepEqual(stored.photo.thumbnail, photo.slice(0, 8));

  const read = await client.getCityPhoto({ cityId: "c-00000001" });
  assert.deepEqual(read.photo, photo);
  assert.equal(read.caption, "harbour");
  const empty = await client.getCityPhoto({ cityId: "c-00000001" });
  assert.deepEqual(empty.photo, new Uint8Array());

  await assert.rejects(
    client.putCityPhoto({ cityId: "c-00000001", photo: "text" as unknown as Uint8Array }),
    /Uint8Array/,
  );
});

test("a blob in a JSON body is standard padded base64", () => {
  const bytes = new Uint8Array([0xff, 0xd8, 0x7b, 0x22, 0x00, 0x80, 0xfe, 0x01, 0x02]);
  assert.equal(toWire(blob, bytes), "/9h7IgCA/gEC");
  assert.deepEqual(fromWire(blob, "/9h7IgCA/gEC"), bytes);
  const large = new Uint8Array(100_000).map((_, i) => i % 251);
  assert.deepEqual(fromWire(blob, toWire(blob, large)), large);
});

test("enums are values and types", () => {
  const sky: Sky = Sky.CLOUDY;
  assert.equal(sky, "cloudy");
});

test("malformed modeled fields retain the service base and retry metadata", async () => {
  const { fetch } = fake(json(429, { __type: "ThrottlingError", message: 42 }, { "x-request-id": "base-id" }));
  const error = await new WeatherClient({ fetch, retry: false }).createCity({ name: "Oslo" }).catch(e => e);
  assert.ok(error instanceof WeatherException);
  assert.ok(error instanceof ServiceError);
  assert.equal(error.$metadata.requestId, "base-id");
  assert.equal(error.$metadata.status, 429);
  assert.deepEqual(error.$retryable, { throttling: true });
});
