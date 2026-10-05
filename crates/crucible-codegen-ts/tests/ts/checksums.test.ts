import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { Algorithm, ChecksumError, ChecksumsClient } from "./generated-checksums/src/index.js";
import { computeChecksum, createHasher } from "./generated-checksums/src/checksums.js";
// Each generated package carries its own runtime, so its own error class.
import { ByteStream, ChecksumError as StreamChecksumError, ChecksumStreamingClient } from "./generated-checksum-streaming/src/index.js";

const vectors: { inputUtf8: string; base64: Record<string, string>; emptyMd5: string } =
  JSON.parse(readFileSync(new URL("../../../../../tests/conformance/checksums.json", import.meta.url), "utf8"));
const data = new TextEncoder().encode(vectors.inputUtf8);
const endpoint = "https://checksums.test";
const base64 = (bytes: Uint8Array) => btoa(Array.from(bytes, byte => String.fromCharCode(byte)).join(""));
const digest = (algorithm: string, bytes: Uint8Array) => { const hasher = createHasher(algorithm); hasher.update(bytes); return base64(hasher.digest()); };
function chunks(...parts: string[]): ReadableStream<Uint8Array> {
  return new ReadableStream({ start(controller) { for (const part of parts) controller.enqueue(new TextEncoder().encode(part)); controller.close(); } });
}
function random(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  for (let at = 0; at < length; at += 65536) globalThis.crypto.getRandomValues(bytes.subarray(at, at + 65536));
  return bytes;
}

test("every algorithm matches the shared vectors, in one pass and byte by byte", async () => {
  assert.equal(Object.keys(vectors.base64).length, 7);
  for (const [algorithm, expected] of Object.entries(vectors.base64)) {
    assert.equal(await computeChecksum(algorithm, data), expected, algorithm);
    assert.equal(digest(algorithm, data), expected, algorithm);
    const hasher = createHasher(algorithm);
    for (const byte of data) hasher.update(new Uint8Array([byte]));
    assert.equal(base64(hasher.digest()), expected, algorithm);
  }
  assert.equal(await computeChecksum("MD5", new Uint8Array()), vectors.emptyMd5);
  assert.equal(digest("CRC32", new TextEncoder().encode("The quick brown fox jumps over the lazy dog")), base64(new Uint8Array([0x41, 0x4f, 0xa3, 0x39])));
  for (const unsupported of ["XXHASH64", "XXHASH3", "XXHASH128"]) assert.throws(() => createHasher(unsupported), /unsupported checksum algorithm/);
});

test("the bundled digests agree with platform implementations at every padding boundary", async () => {
  const names: Record<string, string> = { MD5: "md5", SHA1: "sha1", SHA256: "sha256", SHA512: "sha512" };
  const inputs = [...Array.from({ length: 300 }, (_, length) => random(length)), random(1 << 20)];
  for (const input of inputs) {
    for (const [algorithm, node] of Object.entries(names)) {
      const expected = base64(createHash(node).update(input).digest());
      assert.equal(digest(algorithm, input), expected, `${algorithm} of ${input.length} bytes`);
      assert.equal(await computeChecksum(algorithm, input), expected);
    }
  }
  // Uneven chunking agrees with one pass, for the CRCs as for the hashes.
  const large = inputs.at(-1)!;
  for (const algorithm of Object.keys(vectors.base64)) {
    const hasher = createHasher(algorithm);
    for (let at = 0, step = 1; at < large.length; at += step, step = step * 7 % 4099 + 1) hasher.update(large.subarray(at, at + step));
    assert.equal(base64(hasher.digest()), digest(algorithm, large), algorithm);
  }
});

test("request checksums, private defaults, response validation and caller-supplied headers", async () => {
  let mode = "normal";
  const observed: Request[] = [];
  const verified: string[] = [];
  const fetch = async (request: Request) => {
    observed.push(request);
    const body = new Uint8Array(await request.arrayBuffer());
    const algorithm = request.headers.get("x-algorithm");
    if (mode !== "override" && mode !== "skip") {
      const name = algorithm ? `x-amz-checksum-${algorithm.toLowerCase()}` : "content-md5";
      assert.equal(request.headers.get(name), body.length ? vectors.base64[algorithm ?? "MD5"] : vectors.emptyMd5, name);
    }
    const headers = new Headers({ "x-amz-checksum-xxhash3": "unmodeled" });
    const selected = algorithm ?? "CRC32";
    if (mode !== "unmodeled") headers.set(`x-amz-checksum-${selected.toLowerCase()}`, mode === "bad" || mode === "skip" ? "AAAA" : vectors.base64[selected]);
    if (mode === "one") headers.set("x-amz-checksum-sha256", "AAAA");
    return new Response(data, { headers });
  };
  const client = new ChecksumsClient({ endpoint, fetch, checksumValidated: (operation, algorithm) => verified.push(`${operation}:${algorithm}`) });
  await client.legacy({ data });
  await client.required({ data });
  await client.legacy({});
  assert.deepEqual(verified, [], "operations without response checksums verify nothing");

  const input = { data };
  const output = await client.flexible(input);
  assert.deepEqual(output.data, data);
  assert.deepEqual(Object.keys(input), ["data"], "defaults never reach the caller's input");
  assert.equal(observed.at(-1)!.headers.get("x-algorithm"), "CRC32");
  assert.equal(observed.at(-1)!.headers.get("x-validation"), "ENABLED");
  assert.equal(verified.at(-1), "Flexible:CRC32");
  for (const algorithm of Object.keys(vectors.base64)) {
    await client.flexible({ data, algorithm: algorithm as Algorithm });
    assert.equal(verified.at(-1), `Flexible:${algorithm}`);
  }

  mode = "bad";
  const mismatch = await client.flexible(input).catch((error: unknown) => error);
  assert.ok(mismatch instanceof ChecksumError);
  assert.equal(mismatch.algorithm, "CRC32");
  mode = "one";
  await client.flexible(input);
  assert.equal(verified.at(-1), "Flexible:CRC32", "only the first modeled checksum present is checked");
  mode = "unmodeled";
  const count = verified.length;
  await client.flexible(input);
  assert.equal(verified.length, count, "an unmodeled checksum is not validated");

  mode = "override";
  await client.flexible({ data, checksum: "supplied" });
  assert.equal(observed.at(-1)!.headers.get("x-amz-checksum-crc32"), "supplied");
  await client.flexible({ data }, { headers: { "x-amz-checksum-crc32": "from-options" } });
  assert.equal(observed.at(-1)!.headers.get("x-amz-checksum-crc32"), "from-options");
  const sent = observed.length;
  await assert.rejects(client.flexible({ data, algorithm: Algorithm.XXHASH64 }), /unsupported checksum algorithm: XXHASH64/);
  assert.equal(observed.length, sent, "an unsupported algorithm fails before dispatch");

  mode = "skip";
  const optOut = new ChecksumsClient({ endpoint, fetch, requestChecksumCalculation: "WHEN_REQUIRED", responseChecksumValidation: "WHEN_REQUIRED" });
  await optOut.flexible({ data });
  for (const header of ["x-algorithm", "x-validation", "x-amz-checksum-crc32"]) assert.equal(observed.at(-1)!.headers.get(header), null, header);
  assert.throws(() => new ChecksumsClient({ endpoint, requestChecksumCalculation: "ALWAYS" as never }), /checksum policies/);

  // Computed over the final bytes, after caller middleware changed them...
  mode = "normal";
  const rewrite = new ChecksumsClient({ endpoint, fetch, middleware: [(request, next) => next(new Request(request, { body: vectors.inputUtf8 }))] });
  await rewrite.legacy({});
  // ...and never over a header the middleware supplied.
  mode = "override";
  const supplied = new ChecksumsClient({ endpoint, fetch, middleware: [(request, next) => { request.headers.set("content-md5", "supplied"); return next(request); }] });
  await supplied.legacy({ data });
  assert.equal(observed.at(-1)!.headers.get("content-md5"), "supplied");
});

test("streamed uploads get no default checksum and need a caller-supplied one", async () => {
  const seen: Request[] = [];
  const client = new ChecksumStreamingClient({ endpoint, fetch: async request => {
    seen.push(request); await request.arrayBuffer(); return new Response(null, { status: 204 });
  } });
  await client.upload({ body: new ByteStream(chunks("1234", "56789")) });
  assert.equal(seen[0].headers.get("x-algorithm"), null);
  assert.equal(seen[0].headers.get("x-amz-checksum-crc32"), null);

  let cancelled = false;
  const unread = new ByteStream(new ReadableStream<Uint8Array>({ cancel() { cancelled = true; } }));
  await assert.rejects(client.upload({ body: unread, algorithm: "SHA256" }), /supply the x-amz-checksum-sha256 header/);
  assert.equal(seen.length, 1);
  assert.equal(cancelled, true, "the refused stream is released");

  await client.upload({ body: new ByteStream(chunks("123456789")), algorithm: "SHA256", checksum: vectors.base64.SHA256 });
  assert.equal(seen[1].headers.get("x-algorithm"), "SHA256");
  assert.equal(seen[1].headers.get("x-amz-checksum-sha256"), vectors.base64.SHA256);

  // A required checksum still defaults to CRC32 for a stream, so it must be supplied.
  await assert.rejects(client.requiredUpload({ body: new ByteStream(chunks("x")) }), /supply the x-amz-checksum-crc32 header/);
  await client.requiredUpload({ body: new ByteStream(chunks("123456789")) }, { headers: { "x-amz-checksum-crc32": vectors.base64.CRC32 } });
  assert.equal(seen.at(-1)!.headers.get("x-algorithm"), "CRC32");
  assert.equal(seen.at(-1)!.headers.get("x-amz-checksum-crc32"), vectors.base64.CRC32);
});

test("streamed responses are verified incrementally when they end", async () => {
  const verified: string[] = [];
  let headers: Record<string, string> = {};
  let validation: string | null = null;
  const client = new ChecksumStreamingClient({ endpoint, checksumValidated: (operation, algorithm) => verified.push(`${operation}:${algorithm}`), fetch: async request => {
    validation = request.headers.get("x-validation");
    return new Response(chunks("1234", "5", "6789"), { headers });
  } });
  const read = async (stream: ByteStream) => {
    const reader = stream.open().getReader();
    const bytes: number[] = [];
    try {
      for (;;) { const next = await reader.read(); if (next.done) break; bytes.push(...next.value); }
    } finally { reader.releaseLock(); }
    return new TextDecoder().decode(new Uint8Array(bytes));
  };

  headers = { "x-amz-checksum-sha512": vectors.base64.SHA512, "x-amz-checksum-crc64nvme": vectors.base64.CRC64NVME };
  const output = await client.download();
  assert.equal(validation, "ENABLED");
  assert.deepEqual(verified, [], "nothing is verified before the body is read");
  assert.equal(await read(output.body), "123456789");
  assert.deepEqual(verified, ["Download:CRC64NVME"]);

  headers = { "x-amz-checksum-sha512": vectors.base64.SHA512 };
  assert.equal(await read((await client.download()).body), "123456789");
  assert.deepEqual(verified, ["Download:CRC64NVME", "Download:SHA512"]);

  headers = { "x-amz-checksum-crc64nvme": "AAAAAAAAAAA=" };
  const corrupted = await client.download();
  const error = await read(corrupted.body).catch((error: unknown) => error);
  assert.ok(error instanceof StreamChecksumError);
  assert.equal(error.algorithm, "CRC64NVME");
  assert.equal(verified.length, 2);

  headers = {};
  assert.equal(await read((await client.download()).body), "123456789");
  assert.equal(verified.length, 2, "a response without a checksum is passed through");

  headers = { "x-amz-checksum-crc64nvme": "AAAAAAAAAAA=" };
  const optOut = new ChecksumStreamingClient({ endpoint, responseChecksumValidation: "WHEN_REQUIRED", fetch: async request => {
    validation = request.headers.get("x-validation"); return new Response(chunks("123456789"), { headers });
  } });
  assert.equal(await read((await optOut.download()).body), "123456789");
  assert.equal(validation, null);
});
