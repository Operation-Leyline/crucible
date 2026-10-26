import assert from "node:assert/strict";
import { test } from "node:test";
import { TroubleshootingClient, TroubleshootingException, Denied } from "./generated-error-docs/src/index.js";
import { NotFoundError } from "./generated/src/index.js";

const base = "https://docs.example.test/errors";

for (const [name, status, body, headers, expected] of [
  ["body", 403, { __type: "Denied", message: "denied", detail: "access/needed" }, {}, `${base}/Denied/reason-access%2Fneeded`],
  ["empty", 403, { __type: "Denied", message: "denied", detail: "" }, {}, `${base}/Denied/reason-`],
  ["dot", 403, { __type: "Denied", message: "denied", detail: ".." }, {}, `${base}/Denied/reason-%2E%2E`],
  ["unicode", 403, { __type: "Denied", message: "denied", detail: "café" }, {}, `${base}/Denied/reason-caf%C3%A9`],
  ["absent", 403, { __type: "Denied", message: "denied" }, {}, `${base}/Denied`],
  ["null", 403, { __type: "Denied", message: "denied", detail: null }, {}, `${base}/Denied`],
  ["future", 403, { __type: "Denied", message: "denied", detail: "future" }, {}, `${base}/Denied`],
  ["internal", 403, { __type: "Denied", message: "denied", detail: "secret" }, {}, `${base}/Denied`],
  ["unrelated reason name", 400, { __type: "PlainError", message: "plain", reason: ".." }, {}, `${base}/PlainError`],
  ["header", 409, { __type: "HeaderError", message: "denied", reason: "future" }, { "X-Error-Reason": "access/needed" }, `${base}/HeaderError/reason-access%2Fneeded`],
  ["mapped member", 418, { __type: "MappedReasonError", message: "denied", failureReason: "access/needed" }, {}, `${base}/MappedReasonError/reason-access%2Fneeded`],
  ["payload", 422, "..", { "X-Crucible-Error": "PayloadError" }, `${base}/PayloadError/reason-%2E%2E`],
  ["malformed", 403, { __type: "Denied", message: "denied", detail: 12 }, {}, `${base}/Denied`],
  ["unknown", 403, { __type: "FutureError", message: "future", docUrl: "https://untrusted.test" }, {}, undefined],
] as const) {
  test(`error documentation: ${name}`, async () => {
    const client = new TroubleshootingClient({ endpoint: "https://api.example.test", fetch: async () => new Response(typeof body === "string" ? body : JSON.stringify(body), { status, headers }) });
    await assert.rejects(client.inspect(), (error: unknown) => {
      assert.ok(error instanceof TroubleshootingException);
      assert.equal(error.docUrl, expected);
      if (name === "future") { assert.ok(error instanceof Denied); assert.equal(error.detail, "future"); }
      return true;
    });
  });
}

test("unconfigured errors retain modeled fields and do not expose documentation", () => {
  const error = new NotFoundError({ message: "missing" }, { status: 404, headers: new Headers() });
  assert.ok(!Object.hasOwn(error, "docUrl"));
  // @ts-expect-error A package without errorDocs keeps the original error API.
  void error.docUrl;
});

test("constructing an error does not treat an inherited property as its reason", () => {
  const error = new Denied(Object.create({ detail: "access/needed" }), { status: 403, headers: new Headers() });
  assert.equal(error.docUrl, `${base}/Denied`);
  assert.ok(!Object.hasOwn(error, "detail"));
});
