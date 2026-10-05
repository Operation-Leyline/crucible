import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { blob, fromWire, toWire, parseJson, stringifyJson } from "./generated/src/runtime.js";
import type { Schema } from "./generated/src/runtime.js";
import { S_CitySummary, S_Location, S_Forecast } from "./generated/src/schemas.js";

interface Expectation {
  json: string;
  states?: Record<string, "absent" | "null" | "value">;
  blobHex?: string;
  instant?: { member: string; seconds: string; nanoseconds: number };
}
interface Case {
  id: string; target: string; wire: string; reject?: boolean;
  expect?: Expectation; expectByProfile?: Record<string, Expectation>;
}
const cases: Case[] = JSON.parse(readFileSync(new URL("../../../../../tests/conformance/serialization.json", import.meta.url), "utf8"));
const profile = "javascript-bigint-date";
const schemas: Record<string, Schema | undefined> = {
  "example.weather#CitySummary": S_CitySummary,
  "example.weather#Photo": blob,
  // Open string enum values use the native string representation.
  "example.weather#Sky": undefined,
  "example.weather#Location": S_Location,
  "example.weather#Forecast": S_Forecast,
};

class ExactNumber {
  readonly coefficient: string;
  readonly power: number;
  constructor(text: string) {
    const negative = text.startsWith("-");
    const [coefficient, exponent = "0"] = text.replace(/^-/, "").toLowerCase().split("e");
    const [whole, fraction = ""] = coefficient.split(".");
    const digits = (whole + fraction).replace(/^0+/, "");
    const trimmed = digits.replace(/0+$/, "");
    this.coefficient = digits.length === 0 ? "0" : (negative ? "-" : "") + trimmed;
    this.power = digits.length === 0 ? 0 : Number(exponent) - fraction.length + digits.length - trimmed.length;
  }
}
// Parse comparison numbers from their lexical tokens. Parsing the expectation
// through JS number would hide precisely the rounding these vectors measure.
function exactJson(text: string): unknown {
  JSON.parse(text);
  const tokens = text.match(/"(?:[^"\\]|\\.)*"|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?|true|false|null|[{}\[\],:]/g)!;
  let index = 0;
  const take = (wanted: string): void => { assert.equal(tokens[index++], wanted); };
  const value = (): unknown => {
    const token = tokens[index++];
    if (token === "{") {
      const object: Record<string, unknown> = {};
      if (tokens[index] !== "}") do {
        const key = JSON.parse(tokens[index++]) as string;
        take(":");
        Object.defineProperty(object, key, { value: value(), enumerable: true });
        if (tokens[index] !== ",") break;
        index++;
      } while (true);
      take("}"); return object;
    }
    if (token === "[") {
      const array: unknown[] = [];
      if (tokens[index] !== "]") do {
        array.push(value());
        if (tokens[index] !== ",") break;
        index++;
      } while (true);
      take("]"); return array;
    }
    return /^-?\d/.test(token) ? new ExactNumber(token) : JSON.parse(token);
  };
  const parsed = value(); assert.equal(index, tokens.length); return parsed;
}

test("shared serialization coverage: all 15 cases", () => {
  assert.equal(cases.length, 15);
});

for (const row of cases) test(`serialization conformance: ${row.id}`, () => {
  assert.ok(row.target in schemas, `Unknown corpus target: ${row.target}`);
  const schema = schemas[row.target];
  const decode = (): unknown => fromWire(schema, parseJson(row.wire));
  if (row.reject) {
    let rejected = false;
    try { decode(); } catch { rejected = true; }
    assert.ok(rejected, "Invalid wire value was accepted"); return;
  }
  const native = decode();
  const expect = row.expectByProfile?.[profile] ?? row.expect!;
  assert.deepEqual(exactJson(stringifyJson(toWire(schema, native))), exactJson(expect.json));
  for (const [member, wanted] of Object.entries(expect.states ?? {})) {
    const object = native as Record<string, unknown>;
    const state = !(member in object) ? "absent" : object[member] === null ? "null" : "value";
    assert.equal(state, wanted);
  }
  if (expect.blobHex !== undefined) {
    assert.ok(native instanceof Uint8Array);
    assert.equal(Array.from(native, byte => byte.toString(16).padStart(2, "0")).join(""), expect.blobHex);
  }
  if (expect.instant !== undefined) {
    const instant = (native as Record<string, unknown>)[expect.instant.member];
    assert.ok(instant instanceof Date);
    const milliseconds = instant.getTime();
    const seconds = Math.floor(milliseconds / 1000);
    assert.equal(String(seconds), expect.instant.seconds);
    assert.equal((milliseconds - seconds * 1000) * 1_000_000, expect.instant.nanoseconds);
  }
});
