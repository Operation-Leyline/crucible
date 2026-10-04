// Behaviour of a generated client, end to end against a fake fetch. The
// client under test is generated from crates/crucible-core/tests/fixtures/
// weather.json by tools/test-ts.sh.
import assert from "node:assert/strict";
import { test } from "node:test";
import { ConflictError, NetworkError, ServiceError, Sky, ThrottlingError, WeatherClient } from "./generated/src/index.js";
/** A fetch that records requests and answers from a queue. */
function fake(...responses) {
    const sent = [];
    const fetch = async (request) => {
        const body = request.body === null ? undefined : await request.text();
        sent.push({ method: request.method, url: new URL(request.url), headers: request.headers, body });
        const next = responses.shift();
        if (next === undefined)
            throw new Error("no response queued");
        if (next instanceof Error)
            throw next;
        return next;
    };
    return { sent, fetch };
}
const json = (status, body, headers = {}) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json", ...headers } });
const quick = { maxAttempts: 3, baseDelayMs: 1, maxDelayMs: 2 };
test("labels are encoded, queries repeat, timestamps take their location's format", async () => {
    const { sent, fetch } = fake(json(200, { city: { id: "c-1", name: "Oslo" } }, { "X-Request-Id": "r-1" }));
    const client = new WeatherClient({ fetch, token: "secret" });
    const out = await client.getCity({ cityId: "c/1 2", at: new Date("2026-10-01T12:00:00Z") });
    assert.equal(sent[0].url.href, "https://weather.example/cities/c%2F1%202?at=2026-10-01T12%3A00%3A00.000Z");
    assert.equal(sent[0].headers.get("authorization"), "Bearer secret");
    assert.equal(sent[0].body, undefined);
    assert.equal(out.requestId, "r-1");
    assert.equal(out.city.population, 0, "a missing member with a default takes it");
});
test("list query parameters repeat their key", async () => {
    const { sent, fetch } = fake(json(200, { cities: [] }));
    await new WeatherClient({ fetch }).listCities({ tags: ["a", "b,c"], maxResults: 5 });
    assert.deepEqual(sent[0].url.searchParams.getAll("tag"), ["a", "b,c"]);
    assert.equal(sent[0].url.searchParams.get("maxResults"), "5");
});
test("bodies carry body members only, with a digest and a filled idempotency token", async () => {
    const { sent, fetch } = fake(json(201, { city: { id: "c-1", name: "Oslo", population: 7, updatedAt: "2026-09-21T14:13:20Z", createdAt: 1_790_000_000 } }));
    const input = { name: "Oslo", expectedRevision: 3, location: { coordinates: { latitude: 1, longitude: 2 } } };
    const out = await new WeatherClient({ fetch }).createCity(input);
    const body = JSON.parse(sent[0].body);
    assert.deepEqual(Object.keys(body).sort(), ["clientToken", "location", "name"]);
    assert.match(body.clientToken, /^[0-9a-f-]{36}$/);
    assert.equal(input.clientToken, body.clientToken, "written back for replay");
    assert.equal(sent[0].headers.get("x-expected-revision"), "3");
    assert.equal(sent[0].headers.get("content-type"), "application/json");
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(sent[0].body)));
    const hex = Array.from(digest, b => b.toString(16).padStart(2, "0")).join("");
    assert.equal(sent[0].headers.get("x-amz-content-sha256"), hex);
    assert.ok(out.city.updatedAt instanceof Date);
    assert.equal(out.city.updatedAt.toISOString(), "2026-09-21T14:13:20.000Z");
    assert.equal(out.city.createdAt.toISOString(), "2026-09-21T14:13:20.000Z", "@timestampFormat overrides");
    assert.equal(out.city.population, 7);
});
test("a frozen input gets a token on a copy", async () => {
    const { sent, fetch } = fake(json(201, { city: { id: "c-1", name: "Oslo" } }));
    await new WeatherClient({ fetch }).createCity(Object.freeze({ name: "Oslo" }));
    assert.ok(JSON.parse(sent[0].body).clientToken);
});
test("a retried request reuses its token", async () => {
    const { sent, fetch } = fake(json(503, { message: "busy" }), json(201, { city: { id: "c-1", name: "Oslo" } }));
    await new WeatherClient({ fetch, retry: quick }).createCity({ name: "Oslo" });
    assert.equal(sent.length, 2);
    assert.equal(JSON.parse(sent[0].body).clientToken, JSON.parse(sent[1].body).clientToken);
});
test("errors decode to their class by status when unambiguous", async () => {
    const { fetch } = fake(json(409, { message: "taken" }, { "x-request-id": "req-9" }));
    const error = await new WeatherClient({ fetch }).createCity({ name: "Oslo" }).catch(e => e);
    assert.ok(error instanceof ConflictError);
    assert.ok(error instanceof ServiceError);
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
    const { sent, fetch } = fake(json(429, { message: "slow" }, { "retry-after": "0.001" }), json(429, { message: "slow" }, { "retry-after": "0.001" }), json(429, { message: "slow" }, { "retry-after": "0.001" }));
    const error = await new WeatherClient({ fetch, retry: quick }).ping().catch(e => e);
    assert.equal(sent.length, 3);
    assert.ok(error instanceof ThrottlingError);
    assert.equal(error.retryAfter, 0.001);
});
test("an unknown error keeps status and message", async () => {
    const { fetch } = fake(json(418, { message: "teapot" }));
    const error = await new WeatherClient({ fetch }).ping().catch(e => e);
    assert.ok(error instanceof ServiceError);
    assert.equal(error.name, "UnknownError");
    assert.equal(error.message, "teapot");
});
test("network failures retry when safe, then surface as NetworkError", async () => {
    const { sent, fetch } = fake(new TypeError("fetch failed"), new TypeError("fetch failed"), new TypeError("fetch failed"));
    const error = await new WeatherClient({ fetch, retry: quick }).ping().catch(e => e);
    assert.equal(sent.length, 3);
    assert.ok(error instanceof NetworkError);
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
    const { sent, fetch } = fake(json(200, { cities: [{ id: "c-1", name: "A" }], nextToken: "t1" }), json(200, { cities: [{ id: "c-2", name: "B" }] }));
    const names = [];
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
    const seen = [];
    const home = (request, next) => {
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
test("enums are values and types", () => {
    const sky = Sky.CLOUDY;
    assert.equal(sky, "cloudy");
});
