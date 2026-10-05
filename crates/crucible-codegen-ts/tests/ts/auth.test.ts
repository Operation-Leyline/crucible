import assert from "node:assert/strict";
import { test } from "node:test";
import { AuthHeaderClient } from "./generated-auth-header/src/index.js";
import { AuthQueryClient } from "./generated-auth-query/src/index.js";

test("modeled authentication order and retry refresh select the available identity", async () => {
  let bearer = 0, basic = 0, key = 0, attempts = 0;
  const client = new AuthHeaderClient({ endpoint: "https://auth.test", token: () => { bearer++; return undefined; }, basicCredentials: () => { basic++; return { username: "user", password: `pass${basic}` }; }, apiKey: () => { key++; return "unused"; }, retry: { baseDelayMs: 0 }, fetch: async request => {
    attempts++;
    assert.equal(request.headers.get("authorization"), `Basic ${btoa(`user:pass${attempts}`)}`);
    return new Response(null, { status: attempts === 1 ? 503 : 204 });
  } });
  await client.mixed();
  assert.equal(bearer, 2); assert.equal(basic, 2); assert.equal(key, 0);
});

test("operation auth overrides, optional auth, and explicit credentials bypass unrelated providers", async () => {
  let providers = 0;
  const client = new AuthHeaderClient({ endpoint: "https://auth.test", token: () => { providers++; throw new Error("unrelated provider"); }, basicCredentials: () => { providers++; throw new Error("unrelated provider"); }, fetch: async request => {
    assert.equal(request.headers.get("authorization"), new URL(request.url).pathname === "/mixed" ? "Basic explicit" : null);
    return new Response(null, { status: 204 });
  } });
  await client.public(); await client.mixed({ headers: { authorization: "Basic explicit" } });
  assert.equal(providers, 0);
  const anonymous = new AuthHeaderClient({ endpoint: "https://auth.test", fetch: async () => new Response(null, { status: 204 }) });
  await anonymous.optional(); await assert.rejects(anonymous.basic(), /no identity/);
});

test("API keys bind to modeled headers and query strings without duplicate or overwritten credentials", async () => {
  let providers = 0;
  const header = new AuthHeaderClient({ endpoint: "https://auth.test", apiKey: () => { providers++; return "value"; }, fetch: async request => {
    assert.equal(request.headers.get("x-api-key"), providers === 1 ? "ApiKey value" : "existing");
    return new Response(null, { status: 204 });
  } });
  await header.api();
  const explicit = new AuthHeaderClient({ endpoint: "https://auth.test", headers: { "x-api-key": "existing" }, apiKey: () => { throw new Error("must not resolve"); }, fetch: async request => { assert.equal(request.headers.get("x-api-key"), "existing"); return new Response(null, { status: 204 }); } });
  await explicit.api();
  const query = new AuthQueryClient({ endpoint: "https://auth.test", apiKey: "a b/+", fetch: async request => { assert.equal(new URL(request.url).searchParams.get("key"), "a b/+"); return new Response(null, { status: 204 }); } });
  await query.api();
});

test("basic identities encode UTF-8 and remain covered by the total call deadline", async () => {
  const client = new AuthHeaderClient({ endpoint: "https://auth.test", basicCredentials: { username: "é", password: "p" }, fetch: async request => { assert.equal(request.headers.get("authorization"), "Basic w6k6cA=="); return new Response(null, { status: 204 }); } });
  await client.basic();
  const stuck = new AuthHeaderClient({ endpoint: "https://auth.test", timeoutMs: 5, basicCredentials: () => new Promise(() => {}), fetch: async () => { throw new Error("must not dispatch"); } });
  await assert.rejects(stuck.basic(), /call deadline/);
});
