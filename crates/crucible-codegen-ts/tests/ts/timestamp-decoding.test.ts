import assert from "node:assert/strict";
import { test } from "node:test";
import { fromWire, parseJson, timestamp } from "./generated/src/runtime.js";

test("epoch-seconds timestamps decode to the exact millisecond", () => {
  const schema = timestamp("epoch-seconds");
  // 1090069807.626 * 1000 is 1090069807625.9999 in binary floating point.
  assert.equal((fromWire(schema, 1090069807.626) as Date).getTime(), 1090069807626);
  assert.equal((fromWire(schema, parseJson("1090069807.626")) as Date).getTime(), 1090069807626);
  for (let ms = 1_000_000_000_000; ms < 1_000_000_020_000; ms++) {
    assert.equal((fromWire(schema, ms / 1000) as Date).getTime(), ms);
  }
});
