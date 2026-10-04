// The two Node built-ins the tests use, declared here so the suite needs no
// @types/node install: only TypeScript itself.
declare module "node:test" {
  export function test(name: string, run: () => void | Promise<void>): void;
}

declare module "node:assert/strict" {
  interface Assert {
    (value: unknown, message?: string): asserts value;
    ok(value: unknown, message?: string): asserts value;
    equal(actual: unknown, expected: unknown, message?: string): void;
    deepEqual(actual: unknown, expected: unknown, message?: string): void;
    match(value: string, pattern: RegExp, message?: string): void;
    rejects(promise: Promise<unknown>, pattern?: RegExp, message?: string): Promise<void>;
  }
  const assert: Assert;
  export default assert;
}

declare const process: { env: Record<string, string | undefined> };
