/** Native generated TypeScript adapter for protocol_conformance.py. */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = resolve(process.argv[2]);
const runtime = await import(pathToFileURL(resolve(root, "runtime.js")));
const schemas = await import(pathToFileURL(resolve(root, "schemas.js")));
const text = readFileSync(0, "utf8");
const vector = JSON.parse(text);
// Preserve numeric token precision when converting Smithy fixture values.
const params = runtime.parseJson(text).params;
const operation = schemas[vector.operation.split("#").at(-1)];

function native(schema, value) {
  if (value == null || schema == null) return value;
  if (schema.kind === "structure" || schema.kind === "union") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, native(schema.members[key], item)]));
  }
  if (schema.kind === "list") return value.map(item => native(schema.member, item));
  if (schema.kind === "map") return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, native(schema.value, item)]));
  return runtime.fromWire(schema.kind === "timestamp" ? { ...schema, format: "epoch-seconds" } : schema, value);
}
function modelParams(schema, value) {
  if (value == null || schema == null) return value;
  if (schema.kind === "structure" || schema.kind === "union") {
    return Object.fromEntries(Object.entries(schema.members).filter(([key]) => Object.hasOwn(value, key)).map(([key, child]) => [key, modelParams(child, value[key])]));
  }
  if (schema.kind === "list") return value.map(item => modelParams(schema.member, item));
  if (schema.kind === "map") return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, modelParams(schema.value, item)]));
  return runtime.toWire(schema.kind === "timestamp" ? { ...schema, format: "epoch-seconds" } : schema, value);
}
async function run() {
  if (vector.vendorParams != null || vector.authScheme != null) return { unsupported: "TypeScript adapter needs vendor/auth fixture configuration" };
  if (!operation) return { unsupported: "operation missing from generated package" };
  const endpoint = "https://" + (vector.case.host ?? "example.test");
  let snapshot;
  const captured = new Error("captured request");
  const transport = new runtime.Transport({ endpoint, retry: false, fetch: async request => {
    if (vector.case.kind === "request") {
      const url = new URL(request.url);
      snapshot = { method: request.method, uri: url.pathname, host: url.hostname, queryParams: url.search.slice(1).split("&").filter(Boolean), headers: Object.fromEntries(request.headers), body: await request.text() };
      throw captured;
    }
    const message = vector.case.message;
    return new Response(message.body ?? null, { status: vector.case.code, headers: message.headers });
  } }, { middleware: [], bearer: false }, schemas.errors);
  if (vector.case.kind === "request") {
    try { await transport.send(operation, native(operation.input, params)); } catch (error) { if (error !== captured && error.cause !== captured) throw error; }
    if (!snapshot) throw new Error("client did not dispatch request");
    return snapshot;
  }
  // Response vectors omit operation inputs. Keep the real output/error metadata
  // and supply a neutral request so required labels do not mask response tests.
  const responseOperation = { ...operation, method: "POST", path: [], input: undefined, query: [], queryParams: undefined, headers: [], prefixHeaders: [], body: [], payload: undefined };
  try {
    const output = await transport.send(responseOperation, {});
    return { params: modelParams(operation.output.schema, output), error: false };
  } catch (error) {
    if (!(error instanceof runtime.ServiceError)) throw error;
    const spec = schemas.errors[vector.shape.split("#").at(-1)];
    return { params: modelParams(spec.schema, error), error: true };
  }
}
process.stdout.write(runtime.stringifyJson(await run()) + "\n");
