import assert from "node:assert/strict";
import { test } from "node:test";
import { PreciseDate } from "./generated/src/index.js";
import { Transport, fromWire, parseJson, stringifyJson, structure, timestamp, toWire } from "./generated/src/runtime.js";

const dateTime = timestamp("date-time");
const epoch = timestamp("epoch-seconds");
const schema = structure(() => ({ at: dateTime, epochAt: epoch }), {}, []);

/** One operation that binds the same two timestamps to the body, the query and a header. */
async function echo(input: Record<string, unknown>, wire: { body?: string; query?: string; header?: string; epochQuery?: string; epochHeader?: string } = {}) {
  const seen: Record<string, string | null> = {};
  const transport = new Transport({ endpoint: "https://example.test", fetch: async request => {
    seen.body = await request.text();
    seen.query = new URL(request.url).searchParams.get("at");
    seen.epochQuery = new URL(request.url).searchParams.get("epochAt");
    seen.header = request.headers.get("x-at");
    seen.epochHeader = request.headers.get("x-epoch-at");
    return new Response(seen.body, { headers: {
      "x-at": wire.header ?? seen.header ?? "", "x-epoch-at": wire.epochHeader ?? seen.epochHeader ?? "",
    } });
  } }, { bearer: false, middleware: [] }, {});
  const output = await transport.send<Record<string, unknown>>({
    name: "Echo", method: "POST", path: [], errors: [], retrySafe: true, input: schema, body: ["at", "epochAt"],
    query: [["at", "at", dateTime], ["epochAt", "epochAt", epoch]],
    headers: [["at", "x-at", dateTime], ["epochAt", "x-epoch-at", epoch]],
    output: { body: true, schema, headers: [["at", "x-at", dateTime], ["epochAt", "x-epoch-at", epoch]] },
  }, input);
  return { seen, output };
}

test("a microsecond timestamp round-trips through body, query and header", async () => {
  const at = PreciseDate.fromRfc3339("2026-10-01T12:30:00.123456Z");
  const { seen, output } = await echo({ at, epochAt: at });
  assert.equal(seen.body, '{"at":"2026-10-01T12:30:00.123456Z","epochAt":1790857800.123456}');
  assert.equal(seen.query, "2026-10-01T12:30:00.123456Z");
  assert.equal(seen.header, "2026-10-01T12:30:00.123456Z");
  assert.equal(seen.epochQuery, "1790857800.123456");
  assert.equal(seen.epochHeader, "1790857800.123456");
  assert.ok(output.at instanceof PreciseDate && output.at instanceof Date);
  assert.ok((output.at as PreciseDate).equals(at));
  assert.ok((output.epochAt as PreciseDate).equals(at));
});

test("a nanosecond timestamp round-trips through body, query and header", async () => {
  const at = PreciseDate.fromRfc3339("2026-10-01T12:30:00.123456789Z");
  const { seen, output } = await echo({ at, epochAt: at });
  assert.equal(seen.body, '{"at":"2026-10-01T12:30:00.123456789Z","epochAt":1790857800.123456789}');
  assert.equal(seen.query, "2026-10-01T12:30:00.123456789Z");
  assert.equal(seen.header, "2026-10-01T12:30:00.123456789Z");
  assert.equal(seen.epochQuery, "1790857800.123456789");
  assert.equal(seen.epochHeader, "1790857800.123456789");
  for (const key of ["at", "epochAt"]) assert.equal((output[key] as PreciseDate).getEpochNanos(), at.getEpochNanos(), key);
});

test("decoding reads sub-millisecond digits from the wire text, header and body alike", async () => {
  const { output } = await echo({}, { header: "2026-10-01T12:30:00.000000001Z", epochHeader: "1790857800.000000001" });
  assert.equal((output.at as PreciseDate).getEpochNanos(), 1790857800_000000001n);
  assert.equal((output.epochAt as PreciseDate).getEpochNanos(), 1790857800_000000001n);
  const body = fromWire(schema, parseJson('{"at":"2026-10-01T12:30:00.5Z","epochAt":1790857800.999999999}')) as Record<string, PreciseDate>;
  assert.equal(body.at.getEpochNanos(), 1790857800_500000000n);
  assert.equal(body.epochAt.getEpochNanos(), 1790857800_999999999n);
  // A float would round 1790857800.999999999 to ...801; the token's text does not.
  assert.equal(body.epochAt.getTime(), 1790857800999);
});

test("a plain Date encodes exactly as it always has", async () => {
  const at = new Date("2026-10-01T12:30:00.123Z");
  const { seen } = await echo({ at, epochAt: at });
  assert.equal(seen.body, '{"at":"2026-10-01T12:30:00.123Z","epochAt":1790857800.123}');
  assert.equal(seen.query, "2026-10-01T12:30:00.123Z");
  assert.equal(seen.header, "2026-10-01T12:30:00.123Z");
  assert.equal(seen.epochQuery, "1790857800.123");
  assert.equal(seen.epochHeader, "1790857800.123");
  const whole = new Date("2026-10-01T12:30:00Z");
  assert.equal(stringifyJson(toWire(epoch, whole)), "1790857800");
  assert.equal(stringifyJson(toWire(dateTime, whole)), '"2026-10-01T12:30:00.000Z"');
  // A PreciseDate with no sub-millisecond part prints the same as the Date.
  assert.equal(stringifyJson(toWire(dateTime, new PreciseDate(whole))), '"2026-10-01T12:30:00.000Z"');
  assert.equal(stringifyJson(toWire(epoch, new PreciseDate(at))), "1790857800.123");
});

test("six digits print for whole microseconds and nine otherwise", () => {
  assert.equal(PreciseDate.fromEpochNanos(1_000_000n * 5n + 120_000n).toISOString(), "1970-01-01T00:00:00.005120Z");
  assert.equal(PreciseDate.fromEpochNanos(1_000_000n * 5n + 120_001n).toISOString(), "1970-01-01T00:00:00.005120001Z");
  assert.equal(JSON.stringify({ at: PreciseDate.fromEpochNanos(1n) }), '{"at":"1970-01-01T00:00:00.000000001Z"}');
});

test("the runtime's copy keeps the precision", async () => {
  // A retry resends a snapshot of the caller's input; it must still be exact.
  let attempts = 0;
  const at = PreciseDate.fromEpochNanos(1790857800_123456789n);
  const bodies: string[] = [];
  const transport = new Transport({ endpoint: "https://example.test", retry: { baseDelayMs: 0 }, fetch: async request => {
    bodies.push(await request.text());
    return new Response(null, { status: ++attempts === 1 ? 503 : 204 });
  } }, { bearer: false, middleware: [] }, {});
  await transport.send({ name: "Echo", method: "PUT", path: [], errors: [], retrySafe: true, input: schema, body: ["at"] }, { at });
  assert.equal(attempts, 2);
  assert.deepEqual(bodies, ['{"at":"2026-10-01T12:30:00.123456789Z"}', '{"at":"2026-10-01T12:30:00.123456789Z"}']);
});

test("construction from epoch microseconds and nanoseconds and from text agree", () => {
  const fromMicros = PreciseDate.fromEpochMicros(1790857800_123456n);
  const fromNanos = PreciseDate.fromEpochNanos(1790857800_123456_000n);
  const fromText = new PreciseDate("2026-10-01T12:30:00.123456Z");
  assert.ok(fromMicros.equals(fromNanos) && fromNanos.equals(fromText));
  assert.equal(fromText.getEpochMicros(), 1790857800_123456n);
  assert.equal(fromText.getEpochNanos(), 1790857800_123456_000n);
  assert.equal(PreciseDate.fromRfc3339("2026-10-01T14:30:00.123456+02:00").getEpochMicros(), 1790857800_123456n);
  assert.equal(fromText.getEpochMicros() % 1_000_000n, 123456n);
  // Truncating below the microsecond.
  assert.equal(PreciseDate.fromEpochNanos(1790857800_123456_789n).getEpochMicros(), 1790857800_123456n);
});

test("instants before 1970 keep a non-negative fraction", () => {
  const before = PreciseDate.fromEpochNanos(-1n);
  assert.equal(before.toISOString(), "1969-12-31T23:59:59.999999999Z");
  assert.equal(before.getEpochNanos(), -1n);
  assert.equal(stringifyJson(toWire(epoch, before)), "-0.000000001");
  assert.equal(fromWire(epoch, parseJson("-0.000000001")) instanceof PreciseDate, true);
  assert.equal((fromWire(epoch, parseJson("-1.5")) as PreciseDate).getEpochNanos(), -1_500_000_000n);
});

test("compare and equals see the nanosecond that getTime and operators do not", () => {
  const a = PreciseDate.fromEpochNanos(1_000_000_000n + 1n);
  const b = PreciseDate.fromEpochNanos(1_000_000_000n + 2n);
  assert.equal(a.getTime(), b.getTime());
  assert.equal(a.valueOf(), b.valueOf());
  assert.ok(!(a < b) && !(a > b), "operators see milliseconds");
  assert.equal(a.compare(b), -1);
  assert.equal(b.compare(a), 1);
  assert.equal(a.compare(PreciseDate.fromEpochNanos(1_000_000_001n)), 0);
  assert.ok(!a.equals(b));
  assert.ok(new PreciseDate(1000).equals(new Date(1000)), "a plain Date has no sub-millisecond part");
  assert.equal(a.compare(new Date(1000)), 1);
  assert.ok(Number.isNaN(new PreciseDate(NaN).compare(a)));
});

test("setTime clears the sub-millisecond part and field setters keep it", () => {
  const value = PreciseDate.fromEpochNanos(1_000_000_000n + 123_456n);
  value.setMilliseconds(7);
  assert.equal(value.getEpochNanos(), 1_007_000_000n + 123_456n);
  value.setTime(5_000);
  assert.equal(value.getEpochNanos(), 5_000_000_000n);
});

test("text finer than a nanosecond or not an instant is a TypeError", () => {
  assert.throws(() => new PreciseDate("2026-10-01T12:30:00.1234567891Z"), /timestamp|RFC/);
  assert.throws(() => PreciseDate.fromRfc3339("2026-02-30T00:00:00Z"), /timestamp|RFC/);
  assert.throws(() => PreciseDate.fromRfc3339("not a time"), /timestamp|RFC/);
  assert.throws(() => fromWire(dateTime, "2026-10-01T12:30:00.1234567891Z"), /timestamp|RFC/);
  assert.throws(() => fromWire(epoch, parseJson("1790857800.1234567891")), /timestamp|RFC/);
  // Trailing zeros past nine digits add no precision.
  assert.equal(PreciseDate.fromRfc3339("2026-10-01T12:30:00.1234567890Z").getEpochNanos(), 1790857800_123456789n);
  assert.equal(fromWire(dateTime, "2026-10-01T12:30:00.000Z") instanceof PreciseDate, true);
});

test("an offset that carries the UTC instant outside years 0000 to 9999 is refused", () => {
  assert.throws(() => PreciseDate.fromRfc3339("0000-01-01T00:00:00+01:00"), /outside years 0000 through 9999/);
  assert.throws(() => fromWire(dateTime, "9999-12-31T23:59:59-01:00"), /outside years 0000 through 9999/);
  // The edges themselves are still instants the wire writes.
  assert.equal(PreciseDate.fromRfc3339("0000-01-01T00:00:00Z").toISOString(), "0000-01-01T00:00:00.000Z");
  assert.equal(PreciseDate.fromRfc3339("0000-01-01T01:00:00+01:00").toISOString(), "0000-01-01T00:00:00.000Z");
  assert.equal(PreciseDate.fromRfc3339("9999-12-31T23:59:59-00:00").toISOString(), "9999-12-31T23:59:59.000Z");
});

test("PreciseDate is a Date and a plain Date is still accepted wherever a timestamp is", () => {
  const value = PreciseDate.fromEpochNanos(1790857800_123456789n);
  assert.ok(value instanceof Date);
  assert.equal(value.getTime(), 1790857800123);
  assert.equal(value.toJSON(), "2026-10-01T12:30:00.123456789Z");
  assert.equal(JSON.stringify(new PreciseDate(NaN)), "null");
  assert.equal(PreciseDate.from(new Date(7)).getTime(), 7);
  const copy = PreciseDate.from(value);
  assert.ok(copy !== value);
  assert.ok(copy.equals(value));
});
