// The two Node built-ins the tests use, declared here so the suite needs no
// @types/node install: only TypeScript itself.
declare module "node:test" {
  export function test(name: string, run: () => void | Promise<void>): void;
  export function test(name: string, options: { skip: boolean | string }, run: () => void | Promise<void>): void;
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

declare module "node:crypto" {
  export function createHash(algorithm: string): { update(data: Uint8Array): { digest(): Uint8Array } };
  export function createHmac(algorithm: string, key: Uint8Array | string): { update(data: Uint8Array | string): { digest(): Uint8Array } };
}

// A loopback HTTP/1.1 server for the event stream tests.
declare module "node:http" {
  interface IncomingMessage extends AsyncIterable<Uint8Array> {
    readonly method?: string;
    readonly url?: string;
    readonly headers: Readonly<Record<string, string | string[] | undefined>>;
    pause(): this;
    resume(): this;
    on(event: "close" | "end", listener: () => void): this;
  }
  interface ServerResponse {
    writeHead(status: number, headers?: Readonly<Record<string, string>>): this;
    write(chunk: Uint8Array | string): boolean;
    end(chunk?: Uint8Array | string): this;
    flushHeaders(): void;
    destroy(): void;
    readonly destroyed: boolean;
    once(event: "drain" | "close", listener: () => void): this;
    off(event: "drain" | "close", listener: () => void): this;
    on(event: "close", listener: () => void): this;
  }
  interface Server {
    listen(port: number, host: string, callback: () => void): this;
    address(): { port: number } | string | null;
    close(callback?: () => void): this;
    closeAllConnections(): void;
  }
  export function createServer(handler: (request: IncomingMessage, response: ServerResponse) => void): Server;
  export type { IncomingMessage, ServerResponse, Server };
}
