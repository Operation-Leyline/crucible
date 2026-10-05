// Endpoint rules are lowered and validated by the compiler.
import { query, type Expression as QueryExpression } from "./query.js";

type Step = { readonly kind: "member"; readonly name: string } | { readonly kind: "index"; readonly index: number };
type Part = { readonly kind: "literal"; readonly value: string } | { readonly kind: "reference"; readonly name: string; readonly path: readonly Step[] };
type Expression = { readonly kind: "string"; readonly parts: readonly Part[] }
  | { readonly kind: "boolean" | "integer"; readonly value: boolean | number }
  | { readonly kind: "array"; readonly values: readonly Expression[] }
  | { readonly kind: "object"; readonly values: Readonly<Record<string, Expression>> }
  | { readonly kind: "reference"; readonly name: string }
  | { readonly kind: "call"; readonly function: string; readonly arguments: readonly Expression[] };
type Rule = { readonly conditions: readonly { readonly function: string; readonly arguments: readonly Expression[]; readonly assign: string | null }[]; readonly result:
  { readonly kind: "tree"; readonly rules: readonly Rule[] } | { readonly kind: "error"; readonly message: Expression } |
  { readonly kind: "endpoint"; readonly url: Expression; readonly headers: Readonly<Record<string, readonly Expression[]>>; readonly properties: Readonly<Record<string, Expression>> } };
export interface EndpointRules { readonly version: string; readonly parameters: Readonly<Record<string, { readonly kind: "string" | "boolean" | "stringArray"; readonly required: boolean }>>; readonly rules: readonly Rule[]; }
export type ParameterSource = { readonly kind: "static" | "default"; readonly value: unknown } | { readonly kind: "inputMember"; readonly member: string }
  | { readonly kind: "inputPath"; readonly path: { readonly expression: QueryExpression } } | { readonly kind: "client" | "builtIn"; readonly name: string };
export interface EndpointPrefix { readonly parts: readonly ({ readonly kind: "literal"; readonly value: string } | { readonly kind: "label"; readonly member: string })[]; }
export interface ResolvedEndpoint { readonly url: string; readonly headers: Readonly<Record<string, readonly string[]>>; readonly properties: Readonly<Record<string, unknown>>; }
const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value);

export function hostLabel(value: unknown, subdomains = false): boolean {
  return typeof value === "string" && value.length > 0 && (subdomains ? value.split(".") : [value]).every(label => /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(label));
}
export function parseEndpoint(value: unknown): { scheme: string; authority: string; path: string; normalizedPath: string; isIp: boolean } | null {
  if (typeof value !== "string" || /[\x00-\x20\x7f?#\\]/.test(value) || !/^https?:\/\//.test(value)) return null;
  try {
    const url = new URL(value); if (url.username || url.password) return null;
    const ipv4 = /^\d+\.\d+\.\d+\.\d+$/.test(url.hostname), ipv6 = url.hostname.startsWith("[");
    if (!ipv4 && !ipv6 && !hostLabel(url.hostname.replace(/\.$/, ""), true)) return null;
    return { scheme: url.protocol.slice(0, -1), authority: url.host, path: url.pathname, normalizedPath: url.pathname.endsWith("/") ? url.pathname : url.pathname + "/", isIp: ipv4 || ipv6 };
  } catch { return null; }
}
function attribute(value: unknown, path: unknown): unknown {
  let steps: readonly Step[];
  if (typeof path === "string") {
    steps = Array.from(path.matchAll(/([^.[\]]+)|\[(-?\d+)\]/g), match => match[1] === undefined ? { kind: "index" as const, index: Number(match[2]) } : { kind: "member" as const, name: match[1] });
  } else { steps = path as readonly Step[]; }
  for (const step of steps) value = step.kind === "member" ? object(value) && Object.hasOwn(value, step.name) ? value[step.name] : null : Array.isArray(value) ? value.at(step.index) ?? null : null;
  return value ?? null;
}
function invoke(name: string, args: unknown[]): unknown {
  const [a, b, c, d] = args;
  switch (name) {
    case "isSet": return a != null;
    case "not": if (typeof a !== "boolean") throw new TypeError("endpoint not expects boolean"); return !a;
    case "booleanEquals": return typeof a === "boolean" && typeof b === "boolean" && a === b;
    case "stringEquals": return typeof a === "string" && typeof b === "string" && a === b;
    case "getAttr": return attribute(a, b);
    case "substring": if (typeof a !== "string" || /[^\x00-\x7f]/.test(a) || typeof b !== "number" || typeof c !== "number" || b < 0 || c < b || c > a.length) return null; return d ? a.slice(a.length - c, a.length - b) : a.slice(b, c);
    case "isValidHostLabel": return hostLabel(a, b === true);
    case "parseUrl": return parseEndpoint(a);
    case "uriEncode": if (typeof a !== "string") throw new TypeError("endpoint uriEncode expects string"); return encodeURIComponent(a).replace(/[!'()*]/g, c => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
    default: throw new TypeError(`unknown endpoint function ${name}`);
  }
}
function expression(node: Expression, values: Record<string, unknown>): unknown {
  switch (node.kind) {
    case "reference": return values[node.name] ?? null;
    case "boolean": case "integer": return node.value;
    case "array": return node.values.map(node => expression(node, values));
    case "object": return Object.fromEntries(Object.entries(node.values).map(([key, node]) => [key, expression(node, values)]));
    case "call": return invoke(node.function, node.arguments.map(node => expression(node, values)));
    case "string": return node.parts.map(part => {
      const value = part.kind === "literal" ? part.value : attribute(values[part.name], part.path);
      if (!["string", "number", "boolean"].includes(typeof value)) throw new TypeError("endpoint template references missing or non-scalar value"); return String(value);
    }).join("");
  }
}
function rules(nodes: readonly Rule[], parameters: Record<string, unknown>): ResolvedEndpoint | undefined {
  for (const node of nodes) {
    const scope = { ...parameters }; let matches = true;
    for (const condition of node.conditions) {
      const value = invoke(condition.function, condition.arguments.map(node => expression(node, scope)));
      if (value == null || value === false) { matches = false; break; }
      if (condition.assign !== null) Object.defineProperty(scope, condition.assign, { value, configurable: true, enumerable: true });
    }
    if (!matches) continue;
    const result = node.result;
    if (result.kind === "tree") { const endpoint = rules(result.rules, scope); if (endpoint) return endpoint; }
    else if (result.kind === "error") throw new TypeError(String(expression(result.message, scope)));
    else {
      const url = expression(result.url, scope); if (typeof url !== "string" || !parseEndpoint(url)) throw new TypeError("endpoint rule returned invalid HTTP(S) URL");
      const headers = Object.fromEntries(Object.entries(result.headers).map(([name, nodes]) => [name, nodes.map(node => { const value = expression(node, scope); if (typeof value !== "string") throw new TypeError("endpoint headers must be strings"); return value; })]));
      return { url, headers, properties: Object.fromEntries(Object.entries(result.properties).map(([key, node]) => [key, expression(node, scope)])) };
    }
  }
  return undefined;
}
export function resolveEndpoint(description: EndpointRules, sources: Readonly<Record<string, readonly ParameterSource[]>>, input: Record<string, unknown>, client: Readonly<Record<string, unknown>>, builtins: Readonly<Record<string, unknown>>): ResolvedEndpoint {
  const parameters = Object.create(null) as Record<string, unknown>;
  for (const [name, parameter] of Object.entries(description.parameters)) {
    let value: unknown = null;
    for (const source of sources[name] ?? []) {
      switch (source.kind) {
        case "static": case "default": value = source.value; break;
        case "inputMember": value = input[source.member]; break;
        case "inputPath": value = query(source.path.expression, input); break;
        case "client": value = client[source.name]; break;
        case "builtIn": value = builtins[source.name]; break;
      }
      if (value != null) break;
    }
    if (value == null && parameter.required) throw new TypeError(`required endpoint parameter ${name} is missing`);
    if (value != null && !(parameter.kind === "string" ? typeof value === "string" : parameter.kind === "boolean" ? typeof value === "boolean" : Array.isArray(value) && value.every(item => typeof item === "string"))) throw new TypeError(`endpoint parameter ${name} has the wrong type`);
    parameters[name] = value;
  }
  const result = rules(description.rules, parameters); if (!result) throw new TypeError("no endpoint rule matched"); return result;
}
export function hostPrefix(endpoint: string, description: EndpointPrefix, input: Record<string, unknown>): string {
  const prefix = description.parts.map(part => { if (part.kind === "literal") return part.value; const value = input[part.member]; if (!hostLabel(value)) throw new TypeError(`invalid endpoint host label ${part.member}`); return value; }).join("");
  const url = new URL(endpoint), host = prefix + url.hostname;
  if (host.length > 253 || !hostLabel(host, true)) throw new TypeError("invalid endpoint hostname after prefix expansion");
  url.hostname = host; return url.toString();
}
