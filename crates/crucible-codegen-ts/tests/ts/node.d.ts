// The Node built-ins the tests use, declared here so the suite needs no
// @types/node install: only TypeScript itself.
declare module "node:test" {
  export function test(name: string, run: () => void | Promise<void>): void;
  export function test(name: string, options: { skip: boolean | string }, run: () => void | Promise<void>): void;
}

declare module "node:http" {
  interface ServerResponse {
    writeHead(status: number, headers: Record<string, string>): this;
    write(chunk: string): boolean;
    end(chunk?: string | Uint8Array): this;
    on(event: "close", listener: () => void): this;
  }
  interface Server {
    listen(port: number, host: string, listener: () => void): this;
    address(): { port: number } | string | null;
    close(listener: (error?: Error) => void): this;
    closeAllConnections(): void;
  }
  export function createServer(listener: (request: unknown, response: ServerResponse) => void): Server;
}

declare module "node:util" {
  export function inspect(value: unknown): string;
}

declare module "node:fs" {
  export function readFileSync(path: URL, encoding: "utf8"): string;
}

declare module "node:assert/strict" {
  interface Assert {
    (value: unknown, message?: string): asserts value;
    ok(value: unknown, message?: string): asserts value;
    equal(actual: unknown, expected: unknown, message?: string): void;
    deepEqual(actual: unknown, expected: unknown, message?: string): void;
    match(value: string, pattern: RegExp, message?: string): void;
    throws(run: () => unknown, pattern?: RegExp, message?: string): void;
    rejects(promise: Promise<unknown>, pattern?: RegExp | ((error: unknown) => boolean), message?: string): Promise<void>;
  }
  const assert: Assert;
  export default assert;
}
