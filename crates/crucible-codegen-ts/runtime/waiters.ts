import { ByteStream, ServiceError, abortable, numericText, parseJson, sleep, type RequestOptions } from "./runtime.js";
import { query, type Expression } from "./query.js";

type Matcher = { readonly kind: "success"; readonly expected: boolean } | { readonly kind: "errorType"; readonly name: string }
  | { readonly kind: "output" | "inputOutput"; readonly path: { readonly expression: Expression }; readonly comparator: "stringEquals" | "booleanEquals" | "allStringEquals" | "anyStringEquals"; readonly expected: string };
export interface WaiterSpec {
  readonly minDelay: number; readonly maxDelay: number;
  readonly acceptors: readonly { readonly state: "success" | "failure" | "retry"; readonly matcher: Matcher; readonly message?: { readonly expression: Expression } | null }[];
}
export interface WaiterOptions extends RequestOptions {
  /** Total waiter budget including calls, retries and delays. */
  readonly maxWaitTimeMs: number;
  readonly minDelayMs?: number;
  readonly maxDelayMs?: number;
}
export type WaiterResult<O> = { readonly outcome: "response"; readonly output: O; readonly attempts: number }
  | { readonly outcome: "error"; readonly error: unknown; readonly attempts: number };
export class WaiterError extends Error {
  constructor(readonly waiter: string, readonly attempts: number, readonly response: unknown, readonly cause: unknown, readonly detail?: string | readonly string[]) {
    super(`waiter ${waiter} failed after ${attempts} attempts${detail ? `: ${Array.isArray(detail) ? detail.join(", ") : detail}` : ""}`, { cause }); this.name = "WaiterError";
  }
}
export class WaiterTimeout extends WaiterError { constructor(waiter: string, attempts: number, response: unknown, cause: unknown) { super(waiter, attempts, response, cause, "deadline exceeded"); this.name = "WaiterTimeout"; } }

/** Waiter expressions use model names and timestamps in epoch seconds. */
export function queryValue(value: unknown): unknown {
  if (value instanceof Date) return value.getTime() / 1000;
  if (value instanceof ByteStream) return null;
  if (value === null || typeof value !== "object" || numericText(value) !== undefined) return value;
  if (Array.isArray(value)) return value.map(queryValue);
  const fields = value as Record<string, unknown>;
  if (Object.hasOwn(fields, "$unknown")) {
    const arm = fields.$unknown as { name: string; json: string }; return { [arm.name]: parseJson(arm.json) };
  }
  return Object.fromEntries(Object.entries(fields).map(([name, value]) => [name, queryValue(value)]));
}
async function release(value: unknown): Promise<void> {
  if (value instanceof ByteStream) { await value.cancel().catch(() => {}); return; }
  if (Array.isArray(value)) { await Promise.all(value.map(release)); return; }
  if (value !== null && typeof value === "object" && numericText(value) === undefined) await Promise.all(Object.values(value).map(release));
}
function accept(spec: WaiterSpec, input: unknown, output: unknown, error: unknown, failed: boolean): { state: "success" | "failure" | "retry"; detail?: string | readonly string[] } {
  for (const acceptor of spec.acceptors) {
    const matcher = acceptor.matcher; let matches = false;
    if (matcher.kind === "success") matches = !failed === matcher.expected;
    else if (matcher.kind === "errorType") matches = error instanceof ServiceError && [matcher.name, matcher.name.split("#").at(-1)].includes(error.name);
    else if (!failed) {
      const value = query(matcher.path.expression, matcher.kind === "inputOutput" ? { input, output } : output);
      switch (matcher.comparator) {
        case "stringEquals": matches = typeof value === "string" && value === matcher.expected; break;
        case "booleanEquals": matches = typeof value === "boolean" && value === (matcher.expected === "true"); break;
        case "allStringEquals": matches = Array.isArray(value) && value.length > 0 && value.every(value => typeof value === "string" && value === matcher.expected); break;
        case "anyStringEquals": matches = Array.isArray(value) && value.some(value => typeof value === "string" && value === matcher.expected); break;
      }
    }
    if (matches) {
      const message = acceptor.message ? query(acceptor.message.expression, output) : undefined;
      const detail = typeof message === "string" || (Array.isArray(message) && message.every(value => typeof value === "string")) ? message as string | string[] : undefined;
      return { state: acceptor.state, detail };
    }
  }
  return { state: failed ? "failure" : "retry" };
}
export async function waitFor<I, O>(name: string, spec: WaiterSpec, input: I, call: (input: I, options: RequestOptions) => Promise<O>, options: WaiterOptions): Promise<WaiterResult<O>> {
  const minimum = options.minDelayMs ?? spec.minDelay * 1000, maximum = options.maxDelayMs ?? spec.maxDelay * 1000;
  if (!Number.isFinite(options.maxWaitTimeMs) || options.maxWaitTimeMs <= 0 || !Number.isFinite(minimum) || !Number.isFinite(maximum) || minimum < 0 || maximum < minimum) throw new TypeError("waiter budget must be positive and delays finite, nonnegative, and ordered");
  const controller = new AbortController(); let attempts = 0, output: O | undefined, error: unknown;
  const deadline = performance.now() + options.maxWaitTimeMs;
  const abort = () => controller.abort(options.signal?.reason);
  if (options.signal?.aborted) abort(); else options.signal?.addEventListener("abort", abort, { once: true });
  let timer: ReturnType<typeof setTimeout> | undefined;
  const arm = () => { const remaining = deadline - performance.now(); if (remaining <= 0) controller.abort(new WaiterTimeout(name, attempts, output, error)); else timer = setTimeout(arm, Math.min(remaining, 2_147_483_647)); }; arm();
  try {
    for (;;) {
      controller.signal.throwIfAborted(); attempts++; let failed = false; output = undefined; error = undefined;
      try { output = await abortable(call(input, { ...options, signal: controller.signal }), controller.signal); }
      catch (caught) { controller.signal.throwIfAborted(); error = caught; failed = true; }
      const result = accept(spec, queryValue(input), queryValue(output), error, failed);
      if (result.state === "success") return failed ? { outcome: "error", error, attempts } : { outcome: "response", output: output as O, attempts };
      if (result.state === "failure") throw new WaiterError(name, attempts, output, error, result.detail);
      await abortable(release(output), controller.signal);
      const remaining = deadline - performance.now(), ceiling = Math.min(maximum, minimum * 2 ** Math.min(attempts - 1, 62));
      const delay = minimum + Math.random() * (ceiling - minimum);
      // Preserve a final call's budget when the next delay would consume it.
      const sleepTime = remaining <= minimum ? remaining : remaining - delay <= minimum ? remaining - minimum : delay;
      await sleep(Math.max(0, sleepTime), controller.signal);
    }
  } finally { if (timer !== undefined) clearTimeout(timer); options.signal?.removeEventListener("abort", abort); }
}
