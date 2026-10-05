// crucible TypeScript runtime: HTTP checksums. `Content-MD5` for
// `@httpChecksumRequired`, and the `x-amz-checksum-*` headers of
// `@aws.protocols#httpChecksum`, computed over the bytes actually sent and
// verified over the bytes received. Every algorithm is implemented here,
// because the runtime has no dependencies and Web Crypto has neither CRCs,
// MD5 nor an incremental digest; one-shot SHA digests use Web Crypto when the
// platform has it. Edit it in the crucible repository, never in a generated package.

/** When modeled checksums are computed (requests) or verified (responses). */
export type ChecksumPolicy = "WHEN_SUPPORTED" | "WHEN_REQUIRED";

/** A response checksum differed from the bytes received. */
export class ChecksumError extends Error {
  constructor(readonly algorithm: string) {
    super(`response ${algorithm} checksum mismatch`);
    this.name = "ChecksumError";
  }
}

/** An operation's checksum behaviour, as the Smithy IR settles it. */
export type ChecksumSpec =
  | { readonly kind: "md5" }
  | {
      readonly kind: "awsFlexible";
      readonly requestRequired: boolean;
      readonly requestAlgorithmMember?: string;
      readonly requestAlgorithms?: readonly string[];
      readonly validationModeMember?: string;
      readonly responseAlgorithms: readonly string[];
    };

/** What one call computes and verifies. */
export interface ChecksumPlan {
  readonly algorithm?: string;
  /** The request header the checksum travels in; absent when none is sent. */
  readonly header?: string;
  /** Response checksums to verify, in modeled order; the first one present is checked. */
  readonly response: readonly string[];
}

/** An incremental digest, so a streamed body is checked as it passes. */
export interface Hasher {
  update(bytes: Uint8Array): void;
  digest(): Uint8Array;
}

const ALGORITHMS = ["CRC32", "CRC32C", "CRC64NVME", "MD5", "SHA1", "SHA256", "SHA512"];
const SUBTLE: Readonly<Record<string, string>> = { SHA1: "SHA-1", SHA256: "SHA-256", SHA512: "SHA-512" };

export function checksumSupported(algorithm: unknown): algorithm is string {
  return typeof algorithm === "string" && ALGORITHMS.includes(algorithm);
}

export function checksumHeader(algorithm: string): string {
  return `x-amz-checksum-${algorithm.toLowerCase()}`;
}

/**
 * Resolves the plan for one call. Algorithm and validation-mode members the
 * client fills in on the caller's behalf are written to `values`, the call's
 * private snapshot: the caller's input is never changed. A streamed request
 * body cannot be hashed before it is sent, so it gets no default checksum;
 * a required or chosen one must then arrive in a caller-supplied header.
 */
export function planChecksum(
  spec: ChecksumSpec | undefined,
  values: Record<string, unknown>,
  requestPolicy: ChecksumPolicy,
  responsePolicy: ChecksumPolicy,
  streaming: boolean,
): ChecksumPlan {
  if (spec === undefined) return { response: [] };
  if (spec.kind === "md5") return { algorithm: "MD5", header: "content-md5", response: [] };
  let algorithm: string | undefined;
  let header: string | undefined;
  const member = spec.requestAlgorithmMember;
  if (member !== undefined) {
    let chosen = values[member];
    if (chosen == null && (spec.requestRequired || (requestPolicy === "WHEN_SUPPORTED" && !streaming))) {
      chosen = "CRC32";
      values[member] = chosen;
    }
    if (chosen != null) {
      if (!checksumSupported(chosen)) throw new TypeError(`unsupported checksum algorithm: ${String(chosen)}`);
      algorithm = chosen;
      header = checksumHeader(chosen);
    }
  } else if (spec.requestRequired) {
    algorithm = "MD5";
    header = "content-md5";
  }
  let validate = false;
  const mode = spec.validationModeMember;
  if (mode !== undefined) {
    if (values[mode] == null && responsePolicy === "WHEN_SUPPORTED") values[mode] = "ENABLED";
    validate = values[mode] === "ENABLED";
  }
  return { algorithm, header, response: validate ? spec.responseAlgorithms.filter(checksumSupported) : [] };
}

/** The first modeled response checksum the response carries. */
export function responseChecksum(plan: ChecksumPlan, headers: Headers): { algorithm: string; expected: string } | undefined {
  for (const algorithm of plan.response) {
    const expected = headers.get(checksumHeader(algorithm));
    if (expected !== null) return { algorithm, expected };
  }
  return undefined;
}

/** Compares without an early exit, as the expected value comes from the wire. */
export function sameChecksum(expected: string, actual: string): boolean {
  if (expected.length !== actual.length) return false;
  let difference = 0;
  for (let at = 0; at < expected.length; at++) difference |= expected.charCodeAt(at) ^ actual.charCodeAt(at);
  return difference === 0;
}

/** The base64 checksum of `bytes`. */
export async function computeChecksum(algorithm: string, bytes: Uint8Array): Promise<string> {
  const subtle = globalThis.crypto?.subtle;
  if (Object.hasOwn(SUBTLE, algorithm) && subtle !== undefined) {
    return base64(new Uint8Array(await subtle.digest(SUBTLE[algorithm], bytes as BufferSource)));
  }
  const hasher = createHasher(algorithm);
  hasher.update(bytes);
  return base64(hasher.digest());
}

/**
 * Passes `source` through, verifying its checksum when it ends: a mismatch
 * errors the stream with a `ChecksumError` instead of ending it, so a reader
 * never sees a clean end of corrupted data. Pulls only on demand.
 */
export function verifyingStream(source: ReadableStream<Uint8Array>, algorithm: string, expected: string, verified: () => void): ReadableStream<Uint8Array> {
  const reader = source.getReader();
  const hasher = createHasher(algorithm);
  return new ReadableStream<Uint8Array>({
    async pull(controller) {
      const next = await reader.read();
      if (!next.done) {
        hasher.update(next.value);
        controller.enqueue(next.value);
        return;
      }
      reader.releaseLock();
      if (!sameChecksum(expected, base64(hasher.digest()))) {
        controller.error(new ChecksumError(algorithm));
        return;
      }
      try { verified(); } catch (error) { controller.error(error); return; }
      controller.close();
    },
    cancel(reason) { return reader.cancel(reason); },
  }, { highWaterMark: 0 });
}

export function createHasher(algorithm: string): Hasher {
  switch (algorithm) {
    case "CRC32": return new Crc32(crcTable(0xedb88320));
    case "CRC32C": return new Crc32(crcTable(0x82f63b78));
    case "CRC64NVME": return new Crc64();
    case "MD5": return new Md5();
    case "SHA1": return new Sha1();
    case "SHA256": return new Sha256();
    case "SHA512": return new Sha512();
    default: throw new TypeError(`unsupported checksum algorithm: ${algorithm}`);
  }
}

function base64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

// CRCs: reflected, table driven, initial value and final XOR all ones.

const crcTables = new Map<number, Uint32Array>();
function crcTable(polynomial: number): Uint32Array {
  let table = crcTables.get(polynomial);
  if (table === undefined) {
    table = new Uint32Array(256);
    for (let byte = 0; byte < 256; byte++) {
      let value = byte;
      for (let bit = 0; bit < 8; bit++) value = value & 1 ? (value >>> 1) ^ polynomial : value >>> 1;
      table[byte] = value >>> 0;
    }
    crcTables.set(polynomial, table);
  }
  return table;
}

class Crc32 implements Hasher {
  #value = 0xffffffff;
  constructor(readonly table: Uint32Array) {}
  update(bytes: Uint8Array): void {
    let value = this.#value;
    for (let at = 0; at < bytes.length; at++) value = this.table[(value ^ bytes[at]) & 0xff] ^ (value >>> 8);
    this.#value = value;
  }
  digest(): Uint8Array {
    const out = new Uint8Array(4);
    new DataView(out.buffer).setUint32(0, (this.#value ^ 0xffffffff) >>> 0);
    return out;
  }
}

// CRC-64/NVME (reflected 0x9A6C9329AC4BC9B5) in 32-bit halves: BigInt per byte is too slow for bodies.
let crc64Tables: [Uint32Array, Uint32Array] | undefined;
class Crc64 implements Hasher {
  #high = 0xffffffff;
  #low = 0xffffffff;
  update(bytes: Uint8Array): void {
    const [highs, lows] = crc64Tables ??= crc64Table();
    let high = this.#high, low = this.#low;
    for (let at = 0; at < bytes.length; at++) {
      const index = (low ^ bytes[at]) & 0xff;
      low = ((low >>> 8) | (high << 24)) ^ lows[index];
      high = (high >>> 8) ^ highs[index];
    }
    this.#high = high; this.#low = low;
  }
  digest(): Uint8Array {
    const out = new Uint8Array(8);
    const view = new DataView(out.buffer);
    view.setUint32(0, (this.#high ^ 0xffffffff) >>> 0);
    view.setUint32(4, (this.#low ^ 0xffffffff) >>> 0);
    return out;
  }
}
function crc64Table(): [Uint32Array, Uint32Array] {
  const highs = new Uint32Array(256), lows = new Uint32Array(256);
  for (let byte = 0; byte < 256; byte++) {
    let high = 0, low = byte;
    for (let bit = 0; bit < 8; bit++) {
      const carry = low & 1;
      low = (low >>> 1) | (high << 31); high >>>= 1;
      if (carry) { high ^= 0x9a6c9329; low ^= 0xac4bc9b5; }
    }
    highs[byte] = high >>> 0; lows[byte] = low >>> 0;
  }
  return [highs, lows];
}

/** Merkle–Damgård framing shared by MD5 and the SHA family. */
abstract class BlockHash implements Hasher {
  readonly #block: Uint8Array;
  #filled = 0;
  #length = 0;
  constructor(readonly size: number, readonly littleEndian: boolean) { this.#block = new Uint8Array(size); }
  protected abstract compress(view: DataView): void;
  protected abstract output(): Uint8Array;
  update(bytes: Uint8Array): void {
    this.#length += bytes.length;
    let at = 0;
    while (at < bytes.length) {
      const take = Math.min(this.size - this.#filled, bytes.length - at);
      this.#block.set(bytes.subarray(at, at + take), this.#filled);
      this.#filled += take; at += take;
      if (this.#filled === this.size) { this.compress(new DataView(this.#block.buffer)); this.#filled = 0; }
    }
  }
  digest(): Uint8Array {
    const lengthBytes = this.size / 8;
    const bits = BigInt(this.#length) * 8n;
    const padding = new Uint8Array(((this.size - lengthBytes - this.#filled - 1) % this.size + this.size) % this.size + 1 + lengthBytes);
    padding[0] = 0x80;
    for (let at = 0; at < lengthBytes; at++) {
      const shift = BigInt(8 * (this.littleEndian ? at : lengthBytes - 1 - at));
      padding[padding.length - lengthBytes + at] = Number((bits >> shift) & 0xffn);
    }
    this.update(padding);
    return this.output();
  }
}

function words(values: readonly number[], littleEndian: boolean): Uint8Array {
  const out = new Uint8Array(values.length * 4);
  const view = new DataView(out.buffer);
  values.forEach((value, at) => view.setUint32(at * 4, value >>> 0, littleEndian));
  return out;
}
const rotl = (value: number, bits: number) => (value << bits) | (value >>> (32 - bits));
const rotr = (value: number, bits: number) => (value >>> bits) | (value << (32 - bits));

const MD5_K = [
  0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
  0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
  0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
  0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
  0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
  0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
  0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
  0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];
const MD5_S = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];

class Md5 extends BlockHash {
  readonly #state = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
  constructor() { super(64, true); }
  protected compress(view: DataView): void {
    let [a, b, c, d] = this.#state;
    for (let i = 0; i < 64; i++) {
      let f: number, g: number;
      if (i < 16) { f = (b & c) | (~b & d); g = i; }
      else if (i < 32) { f = (d & b) | (~d & c); g = (5 * i + 1) % 16; }
      else if (i < 48) { f = b ^ c ^ d; g = (3 * i + 5) % 16; }
      else { f = c ^ (b | ~d); g = (7 * i) % 16; }
      f = (f + a + MD5_K[i] + view.getUint32(g * 4, true)) | 0;
      a = d; d = c; c = b;
      b = (b + rotl(f, MD5_S[(i >> 4) * 4 + (i % 4)])) | 0;
    }
    const s = this.#state;
    s[0] = (s[0] + a) | 0; s[1] = (s[1] + b) | 0; s[2] = (s[2] + c) | 0; s[3] = (s[3] + d) | 0;
  }
  protected output(): Uint8Array { return words(this.#state, true); }
}

class Sha1 extends BlockHash {
  readonly #state = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];
  readonly #w = new Int32Array(80);
  constructor() { super(64, false); }
  protected compress(view: DataView): void {
    const w = this.#w;
    for (let t = 0; t < 16; t++) w[t] = view.getUint32(t * 4);
    for (let t = 16; t < 80; t++) w[t] = rotl(w[t - 3] ^ w[t - 8] ^ w[t - 14] ^ w[t - 16], 1);
    let [a, b, c, d, e] = this.#state;
    for (let t = 0; t < 80; t++) {
      const [f, k] = t < 20 ? [(b & c) | (~b & d), 0x5a827999]
        : t < 40 ? [b ^ c ^ d, 0x6ed9eba1]
        : t < 60 ? [(b & c) | (b & d) | (c & d), 0x8f1bbcdc]
        : [b ^ c ^ d, 0xca62c1d6];
      const temp = (rotl(a, 5) + f + e + k + w[t]) | 0;
      e = d; d = c; c = rotl(b, 30); b = a; a = temp;
    }
    const s = this.#state;
    s[0] = (s[0] + a) | 0; s[1] = (s[1] + b) | 0; s[2] = (s[2] + c) | 0; s[3] = (s[3] + d) | 0; s[4] = (s[4] + e) | 0;
  }
  protected output(): Uint8Array { return words(this.#state, false); }
}

// SHA-2 constants are the fractional bits of prime square and cube roots
// (FIPS 180-4 §4.2, §5.3), derived exactly rather than transcribed.
function primes(count: number): bigint[] {
  const found: bigint[] = [];
  for (let candidate = 2; found.length < count; candidate++) {
    if (found.every(prime => BigInt(candidate) % prime !== 0n)) found.push(BigInt(candidate));
  }
  return found;
}
function integerRoot(value: bigint, degree: bigint): bigint {
  let root = 1n << (BigInt(value.toString(2).length) / degree + 1n);
  for (;;) {
    const next = ((degree - 1n) * root + value / root ** (degree - 1n)) / degree;
    if (next >= root) break;
    root = next;
  }
  while (root ** degree > value) root--;
  while ((root + 1n) ** degree <= value) root++;
  return root;
}
/** The first `bits` fractional bits of each prime's `degree`th root. */
function roots(count: number, degree: bigint, bits: bigint): bigint[] {
  const mask = (1n << bits) - 1n;
  return primes(count).map(prime => integerRoot(prime << (bits * degree), degree) & mask);
}
let sha256Constants: { k: Int32Array; h: number[] } | undefined;
let sha512Constants: { k: Int32Array; h: number[] } | undefined;
/** 64-bit constants as interleaved signed 32-bit halves: high, low. */
function halves(values: bigint[]): number[] {
  return values.flatMap(value => [Number(value >> 32n) | 0, Number(value & 0xffffffffn) | 0]);
}

class Sha256 extends BlockHash {
  readonly #state: number[];
  readonly #w = new Int32Array(64);
  readonly #k: Int32Array;
  constructor() {
    super(64, false);
    sha256Constants ??= { k: Int32Array.from(roots(64, 3n, 32n), Number), h: roots(8, 2n, 32n).map(value => Number(value) | 0) };
    this.#k = sha256Constants.k; this.#state = [...sha256Constants.h];
  }
  protected compress(view: DataView): void {
    const w = this.#w, k = this.#k;
    for (let t = 0; t < 16; t++) w[t] = view.getUint32(t * 4);
    for (let t = 16; t < 64; t++) {
      const s0 = rotr(w[t - 15], 7) ^ rotr(w[t - 15], 18) ^ (w[t - 15] >>> 3);
      const s1 = rotr(w[t - 2], 17) ^ rotr(w[t - 2], 19) ^ (w[t - 2] >>> 10);
      w[t] = (w[t - 16] + s0 + w[t - 7] + s1) | 0;
    }
    let [a, b, c, d, e, f, g, h] = this.#state;
    for (let t = 0; t < 64; t++) {
      const t1 = (h + (rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25)) + ((e & f) ^ (~e & g)) + k[t] + w[t]) | 0;
      const t2 = ((rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)) + ((a & b) ^ (a & c) ^ (b & c))) | 0;
      h = g; g = f; f = e; e = (d + t1) | 0; d = c; c = b; b = a; a = (t1 + t2) | 0;
    }
    const s = this.#state;
    [a, b, c, d, e, f, g, h].forEach((value, at) => { s[at] = (s[at] + value) | 0; });
  }
  protected output(): Uint8Array { return words(this.#state, false); }
}

/** SHA-512 over pairs of 32-bit halves, as JavaScript has no 64-bit integers outside BigInt. */
class Sha512 extends BlockHash {
  readonly #state: number[];
  readonly #w = new Int32Array(160);
  readonly #k: Int32Array;
  constructor() {
    super(128, false);
    sha512Constants ??= { k: Int32Array.from(halves(roots(80, 3n, 64n))), h: halves(roots(8, 2n, 64n)) };
    this.#k = sha512Constants.k; this.#state = [...sha512Constants.h];
  }
  protected compress(view: DataView): void {
    const w = this.#w, k = this.#k;
    for (let at = 0; at < 32; at++) w[at] = view.getUint32(at * 4);
    for (let t = 16; t < 80; t++) {
      const [s0h, s0l] = xor3(rotr64(w[2 * t - 30], w[2 * t - 29], 1), rotr64(w[2 * t - 30], w[2 * t - 29], 8), shr64(w[2 * t - 30], w[2 * t - 29], 7));
      const [s1h, s1l] = xor3(rotr64(w[2 * t - 4], w[2 * t - 3], 19), rotr64(w[2 * t - 4], w[2 * t - 3], 61), shr64(w[2 * t - 4], w[2 * t - 3], 6));
      const [high, low] = add64(w[2 * t - 32], w[2 * t - 31], s0h, s0l, w[2 * t - 14], w[2 * t - 13], s1h, s1l);
      w[2 * t] = high; w[2 * t + 1] = low;
    }
    const v = [...this.#state];
    for (let t = 0; t < 80; t++) {
      const [ah, al, bh, bl, ch, cl, dh, dl, eh, el, fh, fl, gh, gl, hh, hl] = v;
      const [S1h, S1l] = xor3(rotr64(eh, el, 14), rotr64(eh, el, 18), rotr64(eh, el, 41));
      const chh = (eh & fh) ^ (~eh & gh), chl = (el & fl) ^ (~el & gl);
      const [t1h, t1l] = add64(hh, hl, S1h, S1l, chh, chl, k[2 * t], k[2 * t + 1], w[2 * t], w[2 * t + 1]);
      const [S0h, S0l] = xor3(rotr64(ah, al, 28), rotr64(ah, al, 34), rotr64(ah, al, 39));
      const majh = (ah & bh) ^ (ah & ch) ^ (bh & ch), majl = (al & bl) ^ (al & cl) ^ (bl & cl);
      const [t2h, t2l] = add64(S0h, S0l, majh, majl);
      const [neh, nel] = add64(dh, dl, t1h, t1l);
      const [nah, nal] = add64(t1h, t1l, t2h, t2l);
      v.splice(0, 16, nah, nal, ah, al, bh, bl, ch, cl, neh, nel, eh, el, fh, fl, gh, gl);
    }
    const s = this.#state;
    for (let at = 0; at < 16; at += 2) [s[at], s[at + 1]] = add64(s[at], s[at + 1], v[at], v[at + 1]);
  }
  protected output(): Uint8Array { return words(this.#state, false); }
}

function rotr64(high: number, low: number, bits: number): [number, number] {
  if (bits >= 32) { [high, low] = [low, high]; bits -= 32; }
  if (bits === 0) return [high, low];
  return [(high >>> bits) | (low << (32 - bits)), (low >>> bits) | (high << (32 - bits))];
}
function shr64(high: number, low: number, bits: number): [number, number] {
  return [high >>> bits, (low >>> bits) | (high << (32 - bits))];
}
function xor3(a: [number, number], b: [number, number], c: [number, number]): [number, number] {
  return [a[0] ^ b[0] ^ c[0], a[1] ^ b[1] ^ c[1]];
}
/** Sums (high, low) pairs modulo 2^64; a handful of 32-bit halves is exact in a double. */
function add64(...halves: number[]): [number, number] {
  let high = 0, low = 0;
  for (let at = 0; at < halves.length; at += 2) { high += halves[at] >>> 0; low += halves[at + 1] >>> 0; }
  high += Math.floor(low / 0x100000000);
  return [high | 0, low | 0];
}
