import assert from "node:assert/strict";
import { test } from "node:test";
import { inspect } from "node:util";
import { Problem, type Record } from "./generated-docs/src/index.js";

test("error formatting redacts nested members, sensitive targets and unknown union data", () => {
  const child: Record = { label: "child", secret: "member-secret", token: "target-secret" };
  const record: Record = { label: "visible", children: [child], lookup: { child }, choice: { secret: "union-secret" }, hidden: { value: "hidden-secret" } };
  Object.defineProperty(record, "__proto__", { value: "prototype-secret", enumerable: true });
  const error = new Problem({ message: "message-secret", record, token: "error-secret" }, { status: 400, headers: new Headers() });
  for (const text of [error.toString(), String(error), inspect(error)]) {
    assert.ok(text.includes("Problem("));
    assert.ok(text.includes('label="visible"'));
    assert.ok(text.includes("*** Sensitive Data Redacted ***"));
    for (const secret of ["member-secret", "target-secret", "union-secret", "hidden-secret", "message-secret", "error-secret", "prototype-secret"]) assert.ok(!text.includes(secret), text);
    assert.ok(!text.includes("code="));
  }
  child.children = [child];
  assert.ok(error.toString().includes("<recursive>"));
  const unknown = new Problem({ record: { choice: { $unknown: { name: "future", json: '"unknown-secret"' } }, label: null as unknown as string } }, { status: 400, headers: new Headers() });
  assert.ok(!unknown.toString().includes("unknown-secret"));
  assert.ok(unknown.toString().includes("label=null"));
});
