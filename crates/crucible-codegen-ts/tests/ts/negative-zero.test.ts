import assert from "node:assert/strict";
import { test } from "node:test";
import { fromWire, parseJson, stringifyJson, toWire } from "./generated/src/runtime.js";
import { S_Coordinates } from "./generated/src/schemas.js";
import type { Coordinates } from "./generated/src/index.js";

test("float members keep negative zero on the wire while positive zero stays zero", () => {
  const coordinates: Coordinates = { latitude: -0, longitude: 0 };
  const text = stringifyJson(toWire(S_Coordinates, coordinates));
  assert.equal(text, '{"latitude":-0.0,"longitude":0}');
});

test("a float member read from a response round-trips negative zero", () => {
  const coordinates = fromWire(S_Coordinates, parseJson('{"latitude":-0.0,"longitude":0}')) as Coordinates;
  assert.ok(Object.is(coordinates.latitude, -0));
  const text = stringifyJson(toWire(S_Coordinates, coordinates));
  const roundTrip = fromWire(S_Coordinates, parseJson(text)) as Coordinates;
  assert.ok(Object.is(roundTrip.latitude, -0));
  assert.ok(Object.is(roundTrip.longitude, 0));
});
