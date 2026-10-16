// The TypeScript client of release v1 of the versioned shop, an SDK line
// (docs/api-versions-clients.md, 5c): generated with `--api-version v1`
// from the current model (v4) as v1 sees it, against the shop served over
// real HTTP. tools/test-e2e.sh generates it, starts
// weather-service/examples/versioned_shop.rs and sets SHOP_URL. The server
// refuses a request that names no release, so every call names v1.
import assert from "node:assert/strict";
import { test } from "node:test";

import { ItemNotFoundException, ShopClient } from "../generated/shop-v1-ts/src/index.js";
import * as shop from "../generated/shop-v1-ts/src/index.js";

const endpoint = process.env.SHOP_URL;
if (!endpoint) throw new Error("SHOP_URL is not set: run tools/test-e2e.sh");

const client = new ShopClient({ endpoint });

test("v1's route and shapes: the item's name, a numeric size, no HUGE", async () => {
  const item = await client.getItem({ id: "big" });
  assert.equal(item.name, "item big");
  assert.equal(item.size, 9);
  assert.equal(item.kind, "LARGE");
});

test("v1 names the page size max, and the server's hook supplies the sort it never sent", async () => {
  const items = await client.listItems({ max: 1 });
  assert.deepEqual(items.names, ["v1", "1", "newest"]);
  assert.deepEqual(items.kinds, ["SMALL", "LARGE"]);
});

test("an operation v3 removed is still the v1 line's", async () => {
  const counted = await client.countItems();
  assert.equal(counted.count, 3);
});

test("operations added since reach the v1 line in its shapes", async () => {
  const labels = await client.listLabels();
  assert.deepEqual(labels.kinds, ["LARGE", "SMALL"]);
  const label = await client.getLabel({ id: "big" });
  assert.deepEqual(label.label, { title: "item big", size: 9, color: "red" });
  // PutLabel, added in v4, takes v1's numeric size and converts it up.
  const put = await client.putLabel({ id: "a", label: { title: "new", size: 3 } });
  assert.deepEqual(put.label, { title: "new", size: 3, color: "v1 for a" });
});

test("v3's new error reaches the v1 line as the one it knows", async () => {
  const gone = await client.getItem({ id: "gone" }).catch(error => error);
  assert.ok(gone instanceof ItemNotFoundException, String(gone));
  assert.equal(gone.message, "sold out");
  assert.equal("ItemGoneException" in shop, false, "the v1 line has no ItemGoneException");
});
