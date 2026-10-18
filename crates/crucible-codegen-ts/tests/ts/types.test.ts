import { test } from "node:test";
import assert from "node:assert/strict";
import { TypesClient, Decimal, Mode, type ReadOutput } from "./generated-types/src/index.js";
import { S_ReadOutput } from "./generated-types/src/schemas.js";
import { Transport, toWire, fromWire, parseJson, stringifyJson, list, map, document, bigDecimal } from "./generated-types/src/runtime.js";

const integer = "12345678901234567890123456789012345678901234567890";
const decimal = "12345678901234567890.12345678901234567890123456789";
test("arbitrary numeric defaults and received values remain exact", async () => {
  const client = new TypesClient({ endpoint: "https://types.test", fetch: async () => new Response("{}") });
  const defaults = await client.read();
  assert.equal(defaults.integer, BigInt(integer)); assert.equal(defaults.decimal.toString(), decimal);
  const values = fromWire(S_ReadOutput, parseJson(`{"integer":-${integer},"decimal":-${decimal},"mode":77,"names":["x","y"],"sparseNames":["x",null],"sparseMap":{"one":${Mode.ON},"missing":null}}`)) as ReadOutput;
  assert.equal(values.integer, -BigInt(integer)); assert.equal(values.decimal.toString(), "-" + decimal); assert.equal(values.mode, 77);
  assert.deepEqual(values.sparseNames, ["x", null]); assert.deepEqual(values.sparseMap, { one: Mode.ON, missing: null });
  const wire = stringifyJson(toWire(S_ReadOutput, values)); assert.ok(wire.includes('"integer":-' + integer)); assert.ok(wire.includes('"decimal":-' + decimal));
});

test("exact decimals survive retry snapshots and bound headers", async () => {
  let count = 0;
  const input = { integer: BigInt(integer), decimal: new Decimal(decimal) };
  const transport = new Transport({ endpoint: "https://types.test", retry: { baseDelayMs: 0 }, fetch: async request => {
    count++; assert.equal(request.headers.get("x-decimal"), decimal); assert.equal(await request.text(), `{"integer":${integer}}`);
    return new Response(null, { status: count === 1 ? 503 : 204 });
  } }, { middleware: [], bearer: false }, {});
  await transport.send({ name: "Exact", method: "POST", path: [], input: S_ReadOutput, body: ["integer"], headers: [["decimal", "x-decimal", bigDecimal]], errors: [], retrySafe: true }, input);
  assert.equal(count, 2);
});

test("set uniqueness uses mathematical numbers and structural object equality", () => {
  for (const text of ['[1,1.0]', '[{"a":1,"b":2},{"b":2,"a":1}]']) {
    let rejected = false;
    try { fromWire(list(document, { unique: true }), parseJson(text)); } catch { rejected = true; }
    assert.equal(rejected, true);
  }
  let rejected = false;
  try { toWire(S_ReadOutput, { integer: 1n, decimal: new Decimal("0"), names: ["same", "same"] }); } catch { rejected = true; }
  assert.equal(rejected, true);
  assert.ok(new Decimal("1.00e2").equals(new Decimal("100")));
});

test("sparse containers accept null and dense documents preserve nested null", () => {
  assert.throws(() => fromWire(list(document), [null]));
  assert.deepEqual(fromWire(list(document), [{ nil: null }]), [{ nil: null }]);
  assert.throws(() => fromWire(map(document), { nil: null }));
  const source = parseJson('{"integer":1e40,"decimal":0.0000000000000000000000000000001,"sparseNames":[null],"sparseMap":{"x":null}}');
  const value = fromWire(S_ReadOutput, source) as ReadOutput;
  assert.equal(value.integer, 10n ** 40n); assert.equal(value.decimal.toString(), "0.0000000000000000000000000000001");
  assert.deepEqual(value.sparseNames, [null]);
});

test("document integers beyond the safe range decode as exact bigint and round-trip", () => {
  const decode = (text: string) => fromWire(document, parseJson(text));
  const wire = (value: unknown) => stringifyJson(toWire(document, value));
  for (const text of ["9007199254740991", "-9007199254740991", "0", "1.5", "-0", "1e3", "1.0", "9007199254740993.5", "1e20"]) {
    assert.equal(typeof decode(text), "number", text);
  }
  assert.ok(Object.is(decode("-0"), -0));
  assert.equal(decode("1e3"), 1000);
  assert.equal(decode("1.5"), 1.5);
  for (const text of ["9007199254740992", "-9007199254740992", "9007199254740993", "-9007199254740993",
    "9223372036854775807", "-9223372036854775808", "18446744073709551615"]) {
    assert.equal(decode(text), BigInt(text), text);
    assert.equal(wire(BigInt(text)), text);
  }
  const nested = fromWire(document, parseJson('{"a":[1,9007199254740993,{"b":-9223372036854775808}],"c":2.5}')) as any;
  assert.equal(nested.a[0], 1);
  assert.equal(nested.a[1], 9007199254740993n);
  assert.equal(nested.a[2].b, -9223372036854775808n);
  assert.equal(nested.c, 2.5);
  assert.equal(wire(nested), '{"a":[1,9007199254740993,{"b":-9223372036854775808}],"c":2.5}');
  const huge = "1" + "0".repeat(400);
  assert.equal(decode(huge), BigInt(huge));
  assert.equal(wire(BigInt(huge)), huge);
  // an undeclared member has no schema: it keeps the plain number
  assert.equal(fromWire(undefined, parseJson("9007199254740993")), 9007199254740992);
  assert.equal(stringifyJson({ a: [1n, {}], b: 9007199254740993n }, 2), JSON.stringify({ a: [1, {}], b: 0 }, null, 2).replace("0\n", "9007199254740993\n"));
  assert.throws(() => JSON.stringify({ n: 1n }));
  assert.throws(() => decode("1e999"));
  assert.throws(() => wire(Number.POSITIVE_INFINITY));
  assert.throws(() => wire(Number.NaN));
});
