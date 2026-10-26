// Executed as ESM inside a real browser page, using the generated SDK's native Fetch.
import { StreamsClient, WorkFailed, StreamProtocolError, TimeoutError, NetworkError } from "/sdk/index.js";

const assert = (condition, detail) => { if (!condition) throw new Error(detail); };
const equal = (value, expected, detail) => assert(JSON.stringify(value) === JSON.stringify(expected), `${detail}: ${JSON.stringify(value)}`);
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

async function withEvents(open, check) {
  let events;
  try { events = await open(); return await check(events); }
  // Lifetime checks must observe release inside check, before this fallback.
  finally { await events?.cancel().catch(() => {}); }
}

async function rejects(action, type, detail) {
  try { await action(); }
  catch (error) { assert(error instanceof type, `${detail}: ${error.name}: ${error.message}`); return error; }
  throw new Error(`${detail}: unexpectedly succeeded`);
}

async function until(check, detail) {
  const deadline = performance.now() + 3000;
  while (performance.now() < deadline) { if (await check()) return; await delay(20); }
  throw new Error(`timed out: ${detail}`);
}

async function json(url) {
  const response = await fetch(url);
  try { assert(response.ok, "fixture stats response"); return await response.json(); }
  finally { await response.body?.cancel().catch(() => {}); }
}

export const stats = endpoint => json(`${endpoint}/browser-stats`);

export function rustClient(endpoint, extra = {}) {
  return new StreamsClient({ endpoint, retry: false, timeoutMs: 5000,
    middleware: [async (request, next) => {
      const headers = new Headers(request.headers);
      headers.set("x-browser-fixture", "native-fetch");
      return next(new Request(request, { headers }));
    }], ...extra });
}

export async function denied(endpoint) {
  const error = await rejects(() => rustClient(endpoint).publish({ topic: "ok" }), NetworkError, "denied CORS origin");
  return { error: error.name };
}

export async function qualify(endpoint) {
  const passed = [];
  const test = async (name, check) => {
    try { await check(); passed.push(name); }
    catch (error) { throw new Error(`${name}: ${error.message}`, { cause: error }); }
  };
  const client = rustClient(endpoint);
  await test("GET stream, native CORS preflight, exposed request id, middleware header", async () => {
    const before = await stats(endpoint);
    await withEvents(() => client.watch(), async events => {
      const values = [];
      for await (const value of events) values.push(value);
      equal(values, [{ ready: "first" }], "GET values");
      assert(events.metadata.status === 200 && !!events.metadata.requestId, "exposed response metadata");
    });
    const after = await stats(endpoint);
    assert(after.preflights > before.preflights, "native browser did not preflight the custom header");
    assert(after.middlewareRequests === before.middlewareRequests + 1, "middleware header was not delivered");
  });
  await test("POST stream and generated typed late error", async () => {
    await withEvents(() => client.publish({ topic: "typed" }), async events => {
      equal((await events.next()).value, { ready: "first" }, "POST first value");
      const error = await rejects(() => events.next(), WorkFailed, "typed late error");
      assert(error.message === "step failed" && error.$metadata.status === 503 && !!error.$metadata.requestId, "typed late error fields");
    });
  });
  await test("normal opening error", async () => {
    const error = await rejects(() => client.publish({ topic: "open-fail" }), WorkFailed, "opening error");
    assert(error.message === "opening failed", "opening error fields");
  });
  await test("unary call still works", async () => equal(await client.read(), { value: "unary" }, "unary result"));
  await test("first item before held EOF and explicit cancellation", async () => {
    const before = await stats(endpoint);
    await withEvents(() => client.publish({ topic: "hold" }, { timeoutMs: null }), async events => {
      equal((await events.next()).value, { ready: "first" }, "progressive first value");
      const active = await stats(endpoint);
      assert(active.polls > before.polls && active.drops === before.drops, "producer must remain open after first value");
      await events.cancel();
      await until(async () => (await stats(endpoint)).drops === before.drops + 1, "cancelled producer drop");
    });
  });
  await test("cancel before consuming opened body", async () => {
    const before = await stats(endpoint);
    await withEvents(() => client.publish({ topic: "hold" }, { timeoutMs: null }), async events => {
      await events.cancel();
      await until(async () => (await stats(endpoint)).drops === before.drops + 1, "unconsumed producer drop after first cancellation");
      await events.cancel();
      equal((await stats(endpoint)).drops, before.drops + 1, "repeated cancellation must not drop another producer");
    });
  });
  await test("abort before opening sends no operation", async () => {
    const before = await stats(endpoint);
    const controller = new AbortController();
    const reason = new Error("before opening");
    controller.abort(reason);
    const error = await rejects(() => client.publish({ topic: "hold" }, { signal: controller.signal, timeoutMs: null }), Error, "pre-aborted call");
    assert(error === reason || error.name === "AbortError", "pre-aborted reason");
    const after = await stats(endpoint);
    equal(after.middlewareRequests, before.middlewareRequests, "pre-aborted operation count");
  });
  await test("abort during pending read releases Rust producer", async () => {
    const before = await stats(endpoint);
    const controller = new AbortController();
    try {
      await withEvents(() => client.publish({ topic: "hold" }, { signal: controller.signal, timeoutMs: null }), async events => {
        await events.next();
        const pending = rejects(() => events.next(), Error, "aborted pending read");
        const reason = new Error("during read");
        controller.abort(reason);
        const error = await pending;
        assert(error === reason || error.name === "AbortError", "pending read abort reason");
        await until(async () => (await stats(endpoint)).drops === before.drops + 1, "aborted producer drop");
      });
    } finally { controller.abort(); }
  });
  await test("loop break releases Rust producer", async () => {
    const before = await stats(endpoint);
    await withEvents(() => client.publish({ topic: "hold" }, { timeoutMs: null }), async events => {
      for await (const value of events) { equal(value, { ready: "first" }, "loop first value"); break; }
      await until(async () => (await stats(endpoint)).drops === before.drops + 1, "loop-break producer drop");
    });
  });
  await test("total deadline remains active after first value", async () => {
    const before = await stats(endpoint);
    await withEvents(() => client.publish({ topic: "hold" }, { timeoutMs: 1000 }), async events => {
      equal((await events.next()).value, { ready: "first" }, "deadline first value");
      await rejects(() => events.next(), TimeoutError, "body deadline");
      await until(async () => (await stats(endpoint)).drops === before.drops + 1, "deadline producer drop");
    });
  });

  const profile = (name, extra = {}) => new StreamsClient({ endpoint: `${location.origin}/profile/${name}`, timeoutMs: 3000, ...extra });
  const collect = async events => { const values = []; for await (const value of events) values.push(value); return values; };
  await test("chunked UTF-8, CRLF, multiline data, comments and ignored replay fields", async () => {
    await withEvents(() => profile("framing").watch(), async events => equal(await collect(events), [{ ready: "a🌊b" }], "framing values"));
  });
  await test("zero-item successful terminal", async () => {
    await withEvents(() => profile("empty").watch(), async events => equal(await collect(events), [], "empty values"));
  });
  await test("unknown application union variant preserved", async () => {
    await withEvents(() => profile("unknown-union").watch(), async events => {
      const [value] = await collect(events);
      equal(value, { $unknown: { name: "future", json: '{"x":"y"}' } }, "unknown union");
    });
  });
  for (const name of ["malformed-json", "malformed-utf8", "truncated", "duplicate-terminal", "late-data", "late-comment", "late-error", "unknown-control"]) {
    await test(`wire rejection: ${name}`, async () => {
      await withEvents(() => profile(name).watch(), async events => {
        const error = await rejects(() => collect(events), StreamProtocolError, name);
        assert(!error.message.includes("private"), "wire error disclosed received payload");
        if (name === "malformed-utf8") assert(error.message.includes("UTF-8"), "invalid UTF-8 must fail decoding");
        if (name === "malformed-json") assert(error.message.includes("JSON"), "invalid JSON must fail parsing");
      });
    });
  }
  await test("UTF-8 byte bound and per-call override", async () => {
    const bounded = profile("large", { maxStreamEventBytes: 256 });
    await withEvents(() => bounded.watch(), async events => {
      const error = await rejects(() => collect(events), StreamProtocolError, "event byte bound");
      assert(error.message.includes("limit"), "byte-bound error");
    });
    await withEvents(() => bounded.watch({ maxStreamEventBytes: 512 }), async events => equal(await collect(events), [{ ready: "🌊".repeat(70) }], "byte override"));
  });
  await test("retry-safe operation never replays after successful opening", async () => {
    const count = async () => (await json("/profile-stats"))["truncated"] ?? 0;
    const before = await count();
    await withEvents(() => profile("truncated", { retry: { maxAttempts: 3, baseDelayMs: 0 } }).watch(), async events => {
      equal((await events.next()).value, { ready: "first" }, "pre-truncation value");
      await rejects(() => events.next(), StreamProtocolError, "interrupted body");
    });
    await delay(100);
    equal(await count(), before + 1, "successful-open request count");
  });
  return passed;
}
