import assert from "node:assert/strict";
import { test } from "node:test";
import { TroubleshootingClient, TroubleshootingException, Denied, DefaultReason, DefaultEmptyReason } from "./generated-error-docs/src/index.js";
import { Transport, document, integer, string, structure } from "./generated-error-docs/src/runtime.js";
import { errors, Inspect } from "./generated-error-docs/src/schemas.js";
import { NotFoundError } from "./generated/src/index.js";

const base = "https://docs.example.test/errors";

for (const [name, status, body, headers, expected] of [
  ["default known", 419, { __type: "DefaultReason", message: "denied" }, {}, `${base}/DefaultReason/reason-access%2Fneeded`],
  ["default empty", 420, { __type: "DefaultEmptyReason", message: "denied" }, {}, `${base}/DefaultEmptyReason/reason-`],
  ["default null", 419, { __type: "DefaultReason", message: "denied", detail: null }, {}, `${base}/DefaultReason`],
  ["default future", 419, { __type: "DefaultReason", message: "denied", detail: "future" }, {}, `${base}/DefaultReason`],
  ["DefaultReason-malformed", 419, "{invalid", { "X-Crucible-Error": "DefaultReason" }, "https://docs.example.test/errors/DefaultReason"],
  ["DefaultReason-scalar", 419, "42", { "X-Crucible-Error": "DefaultReason" }, "https://docs.example.test/errors/DefaultReason"],
  ["DefaultReason-array", 419, "[]", { "X-Crucible-Error": "DefaultReason" }, "https://docs.example.test/errors/DefaultReason"],
  ["DefaultReason-json-null", 419, "null", { "X-Crucible-Error": "DefaultReason" }, "https://docs.example.test/errors/DefaultReason"],
  ["DefaultReason-empty-object", 419, "{}", { "X-Crucible-Error": "DefaultReason" }, "https://docs.example.test/errors/DefaultReason/reason-access%2Fneeded"],
  ["DefaultEmptyReason-malformed", 420, "{invalid", { "X-Crucible-Error": "DefaultEmptyReason" }, "https://docs.example.test/errors/DefaultEmptyReason"],
  ["DefaultEmptyReason-scalar", 420, "42", { "X-Crucible-Error": "DefaultEmptyReason" }, "https://docs.example.test/errors/DefaultEmptyReason"],
  ["DefaultEmptyReason-array", 420, "[]", { "X-Crucible-Error": "DefaultEmptyReason" }, "https://docs.example.test/errors/DefaultEmptyReason"],
  ["DefaultEmptyReason-json-null", 420, "null", { "X-Crucible-Error": "DefaultEmptyReason" }, "https://docs.example.test/errors/DefaultEmptyReason"],
  ["DefaultEmptyReason-empty-object", 420, "{}", { "X-Crucible-Error": "DefaultEmptyReason" }, "https://docs.example.test/errors/DefaultEmptyReason/reason-"],
  ["DefaultReason-empty-body", 419, "", { "X-Crucible-Error": "DefaultReason" }, "https://docs.example.test/errors/DefaultReason/reason-access%2Fneeded"],
  ["DefaultEmptyReason-empty-body", 420, "", { "X-Crucible-Error": "DefaultEmptyReason" }, "https://docs.example.test/errors/DefaultEmptyReason/reason-"],
  ["blob-payload", 421, "not-json /blob", { "X-Crucible-Error": "BlobPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/BlobPayloadError/reason-access%2Fneeded"],
  ["empty-blob-payload", 421, "", { "X-Crucible-Error": "BlobPayloadError", "X-Error-Reason": "" }, "https://docs.example.test/errors/BlobPayloadError/reason-"],
  ["DefaultReason-json-whitespace", 419, " \t\r\n ", { "X-Crucible-Error": "DefaultReason" }, "https://docs.example.test/errors/DefaultReason/reason-access%2Fneeded"],
  ["DefaultReason-vertical-tab", 419, "\u000b", { "X-Crucible-Error": "DefaultReason" }, "https://docs.example.test/errors/DefaultReason"],
  ["DefaultReason-nonbreaking-space", 419, "\u00a0", { "X-Crucible-Error": "DefaultReason" }, "https://docs.example.test/errors/DefaultReason"],
  ["DefaultEmptyReason-json-whitespace", 420, " \t\r\n ", { "X-Crucible-Error": "DefaultEmptyReason" }, "https://docs.example.test/errors/DefaultEmptyReason/reason-"],
  ["DefaultEmptyReason-vertical-tab", 420, "\u000b", { "X-Crucible-Error": "DefaultEmptyReason" }, "https://docs.example.test/errors/DefaultEmptyReason"],
  ["DefaultEmptyReason-nonbreaking-space", 420, "\u00a0", { "X-Crucible-Error": "DefaultEmptyReason" }, "https://docs.example.test/errors/DefaultEmptyReason"],
  ["nonfinite-NaN", 403, "{\"message\":\"denied\",\"detail\":\"access/needed\",\"extra\":NaN}", { "X-Crucible-Error": "Denied" }, "https://docs.example.test/errors/Denied"],
  ["nonfinite-Infinity", 403, "{\"message\":\"denied\",\"detail\":\"access/needed\",\"extra\":Infinity}", { "X-Crucible-Error": "Denied" }, "https://docs.example.test/errors/Denied"],
  ["nonfinite--Infinity", 403, "{\"message\":\"denied\",\"detail\":\"access/needed\",\"extra\":-Infinity}", { "X-Crucible-Error": "Denied" }, "https://docs.example.test/errors/Denied"],
  ["document-object", 423, "{\"value\":1.25}", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError/reason-access%2Fneeded"],
  ["document-array", 423, "[1,true,null]", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError/reason-access%2Fneeded"],
  ["document-number", 423, "42", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError/reason-access%2Fneeded"],
  ["document-null", 423, "null", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError/reason-access%2Fneeded"],
  ["document-string", 423, "\"NaN\"", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError/reason-access%2Fneeded"],
  ["document-empty", 423, "", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError/reason-access%2Fneeded"],
  ["document-whitespace", 423, " \t\r\n", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError/reason-access%2Fneeded"],
  ["document-invalid-nan", 423, "NaN", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError"],
  ["document-invalid-nested-nan", 423, "{\"value\":NaN}", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError"],
  ["document-invalid-infinity", 423, "Infinity", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError"],
  ["document-invalid-vt", 423, "\u000b", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError"],
  ["document-invalid-nbsp", 423, "\u00a0", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError"],
  ["document-invalid-ff", 423, "\f", { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed" }, "https://docs.example.test/errors/DocumentPayloadError"],
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

test("constructed defaulted errors distinguish undefined from null and future values", () => {
  const metadata = { status: 419, headers: new Headers() };
  for (const fields of [{}, { detail: undefined }]) {
    assert.equal(new DefaultReason(fields, metadata).docUrl, `${base}/DefaultReason/reason-access%2Fneeded`);
  }
  for (const detail of [null, "future"]) {
    assert.equal(new DefaultReason({ detail }, metadata).docUrl, `${base}/DefaultReason`);
  }
  assert.equal(new DefaultEmptyReason({}, metadata).docUrl, `${base}/DefaultEmptyReason/reason-`);
});


test("document payload decoding fallback preserves the response diagnostic", async () => {
  // A malformed modeled integer header forces fallback after payload decoding.
  const transport = new Transport({
    endpoint: "https://api.example.test",
    fetch: async () => new Response('{"message":"payload diagnostic"}', {
      status: 423,
      headers: { "X-Crucible-Error": "DocumentPayloadError", "X-Error-Reason": "access/needed", "X-Count": "invalid" },
    }),
  }, { middleware: [], bearer: false }, {
    ...errors,
    DocumentPayloadError: {
      ...errors.DocumentPayloadError,
      headers: [["reason", "X-Error-Reason"], ["count", "X-Count", integer(32)]],
      schema: structure(() => ({ reason: string, count: integer(32), data: document }), {}, []),
    },
  });
  await assert.rejects(transport.send(Inspect, {}), (error: unknown) => {
    assert.ok(error instanceof TroubleshootingException);
    assert.equal(error.docUrl, `${base}/DocumentPayloadError`);
    assert.equal(error.message, "payload diagnostic");
    return true;
  });
});
