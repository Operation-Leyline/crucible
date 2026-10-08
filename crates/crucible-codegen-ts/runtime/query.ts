// Evaluates the compiler's normalized JMESPath AST; no runtime expression parser.
import { Decimal, numericText, parseJson, stringifyJson } from "./runtime.js";

/** A lowered JMESPath expression used by waiter matchers. */
export type Expression =
  | { readonly kind: "identity" }
  | { readonly kind: "field"; readonly name: string }
  | { readonly kind: "index"; readonly index: number }
  | { readonly kind: "literal"; readonly value: unknown }
  | { readonly kind: "subexpression" | "projection" | "and" | "or"; readonly left: Expression; readonly right: Expression }
  | { readonly kind: "flatten" | "objectValues" | "not" | "expressionReference"; readonly expression: Expression }
  | { readonly kind: "slice"; readonly start: number | null; readonly stop: number | null; readonly step: number }
  | { readonly kind: "multiList"; readonly expressions: readonly Expression[] }
  | { readonly kind: "multiHash"; readonly entries: readonly { readonly key: string; readonly value: Expression }[] }
  | { readonly kind: "compare"; readonly comparator: string; readonly left: Expression; readonly right: Expression }
  | { readonly kind: "condition"; readonly predicate: Expression; readonly then: Expression }
  | { readonly kind: "function"; readonly function: string; readonly arguments: readonly Expression[] };

class Reference { constructor(readonly expression: Expression) {} }
const record = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value) && numericText(value) === undefined && !(value instanceof Reference);
const truth = (value: unknown): boolean => value != null && value !== false && value !== "" && (!Array.isArray(value) || value.length > 0) && (!record(value) || Object.keys(value).length > 0);
function number(value: unknown): { coefficient: bigint; exponent: bigint } {
  const text = numericText(value);
  if (text === undefined) throw new TypeError("JMESPath expected a number");
  const [mantissa, exponent = "0"] = text.toLowerCase().split("e");
  const [whole, fraction = ""] = mantissa.split(".");
  return { coefficient: BigInt(whole + fraction), exponent: BigInt(exponent) - BigInt(fraction.length) };
}
function compare(left: unknown, right: unknown): number {
  if (typeof left === "string" && typeof right === "string") {
    const a = Array.from(left, c => c.codePointAt(0)!); const b = Array.from(right, c => c.codePointAt(0)!);
    for (let i = 0; i < Math.min(a.length, b.length); i++) if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
    return Math.sign(a.length - b.length);
  }
  const a = number(left), b = number(right);
  if (a.coefficient === 0n && b.coefficient === 0n) return 0;
  const sign = (value: bigint) => value < 0n ? -1 : value > 0n ? 1 : 0;
  if (sign(a.coefficient) !== sign(b.coefficient)) return sign(a.coefficient) < sign(b.coefficient) ? -1 : 1;
  const ad = a.coefficient.toString().replace("-", ""), bd = b.coefficient.toString().replace("-", "");
  const am = BigInt(ad.length) + a.exponent, bm = BigInt(bd.length) + b.exponent;
  if (am !== bm) return (am < bm ? -1 : 1) * sign(a.coefficient);
  const width = Math.max(ad.length, bd.length), av = ad.padEnd(width, "0"), bv = bd.padEnd(width, "0");
  return (av < bv ? -1 : av > bv ? 1 : 0) * sign(a.coefficient);
}
function equal(a: unknown, b: unknown): boolean {
  if (numericText(a) !== undefined || numericText(b) !== undefined) return numericText(a) !== undefined && numericText(b) !== undefined && compare(a, b) === 0;
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((item, index) => equal(item, b[index]));
  if (record(a) && record(b)) return Object.keys(a).length === Object.keys(b).length && Object.keys(a).every(key => Object.hasOwn(b, key) && equal(a[key], b[key]));
  return a === b;
}
const decimal = (coefficient: bigint, exponent: bigint): Decimal => new Decimal(`${coefficient}e${exponent}`);
function sum(values: unknown[]): Decimal {
  let coefficient = 0n, exponent = 0n;
  for (const value of values) {
    const n = number(value), scale = exponent < n.exponent ? exponent : n.exponent;
    coefficient = coefficient * 10n ** (exponent - scale) + n.coefficient * 10n ** (n.exponent - scale); exponent = scale;
  }
  return decimal(coefficient, exponent);
}
function typed<T>(value: unknown, guard: (value: unknown) => value is T, type: string): T {
  if (!guard(value)) throw new TypeError(`JMESPath expected ${type}`); return value;
}
const array = (value: unknown): unknown[] => typed(value, Array.isArray, "array");
const string = (value: unknown): string => typed(value, (v): v is string => typeof v === "string", "string");
const object = (value: unknown): Record<string, unknown> => typed(value, record, "object");
const reference = (value: unknown): Reference => typed(value, (v): v is Reference => v instanceof Reference, "expression reference");

function invoke(name: string, args: unknown[]): unknown {
  const arity: Record<string, number | readonly [number, number]> = { abs: 1, avg: 1, ceil: 1, contains: 2, ends_with: 2, floor: 1, join: 2, keys: 1, length: 1, map: 2, max: 1, max_by: 2, merge: [1, Infinity], min: 1, min_by: 2, not_null: [1, Infinity], reverse: 1, sort: 1, sort_by: 2, starts_with: 2, sum: 1, to_array: 1, to_number: 1, to_string: 1, type: 1, values: 1 };
  const count = arity[name]; if (count === undefined || (typeof count === "number" ? args.length !== count : args.length < count[0] || args.length > count[1])) throw new TypeError(`JMESPath invalid function arguments: ${name}`);
  const [value, other] = args;
  switch (name) {
    case "abs": { const n = number(value); return decimal(n.coefficient < 0n ? -n.coefficient : n.coefficient, n.exponent); }
    case "ceil": case "floor": { const n = number(value); if (n.exponent >= 0n) return value; const divisor = 10n ** -n.exponent; let integral = n.coefficient / divisor; if (n.coefficient % divisor !== 0n && (name === "ceil" ? n.coefficient > 0n : n.coefficient < 0n)) integral += name === "ceil" ? 1n : -1n; return decimal(integral, 0n); }
    case "sum": return sum(array(value));
    case "avg": { const items = array(value); if (!items.length) return null; const total = number(sum(items)); const precision = 100n; return decimal(total.coefficient * 10n ** precision / BigInt(items.length), total.exponent - precision); }
    case "contains": return typeof value === "string" ? value.includes(string(other)) : array(value).some(item => equal(item, other));
    case "ends_with": return string(value).endsWith(string(other));
    case "starts_with": return string(value).startsWith(string(other));
    case "join": return array(other).map(string).join(string(value));
    case "keys": return Object.keys(object(value));
    case "values": return Object.values(object(value));
    case "length": return typeof value === "string" ? Array.from(value).length : Array.isArray(value) ? value.length : Object.keys(object(value)).length;
    case "map": { const expression = reference(value).expression; return array(other).map(item => query(expression, item)); }
    case "merge": return Object.assign({}, ...args.map(object));
    case "not_null": return args.find(item => item != null) ?? null;
    case "reverse": return typeof value === "string" ? Array.from(value).reverse().join("") : [...array(value)].reverse();
    case "to_array": return Array.isArray(value) ? value : [value];
    case "to_number": if (numericText(value) !== undefined) return value; if (typeof value !== "string") return null; try { const parsed = parseJson(value); return numericText(parsed) !== undefined ? parsed : null; } catch { return null; }
    case "to_string": return typeof value === "string" ? value : stringifyJson(value);
    case "type": return value == null ? "null" : numericText(value) !== undefined ? "number" : Array.isArray(value) ? "array" : value instanceof Reference ? "expref" : typeof value;
    case "min": case "max": case "sort": case "min_by": case "max_by": case "sort_by": {
      const items = array(value), keyed = name.endsWith("_by"), expression = keyed ? reference(other).expression : undefined;
      const entries = items.map(item => ({ item, key: expression ? query(expression, item) : item }));
      let type: "string" | "number" | undefined;
      for (const entry of entries) { const current = typeof entry.key === "string" ? "string" : numericText(entry.key) !== undefined ? "number" : undefined; if (!current || (type !== undefined && current !== type)) throw new TypeError("JMESPath sort keys must be uniformly strings or numbers"); type = current; }
      entries.sort((a, b) => compare(a.key, b.key));
      if (name.startsWith("sort")) return entries.map(entry => entry.item);
      const extreme = name.startsWith("min") ? entries[0] : entries[entries.length - 1];
      return extreme ? entries.find(entry => compare(entry.key, extreme.key) === 0)!.item : null;
    }
    default: throw new TypeError(`unknown JMESPath function ${name}`);
  }
}

/** Evaluate a lowered JMESPath expression against a model value. */
export function query(node: Expression, input: unknown): unknown {
  switch (node.kind) {
    case "identity": return input ?? null;
    case "field": return record(input) && Object.hasOwn(input, node.name) ? input[node.name] ?? null : null;
    case "index": return Array.isArray(input) ? input.at(node.index) ?? null : null;
    case "literal": return node.value;
    case "subexpression": return query(node.right, query(node.left, input));
    case "projection": { const values = query(node.left, input); return Array.isArray(values) ? values.map(value => query(node.right, value)).filter(value => value != null) : null; }
    case "flatten": { const values = query(node.expression, input); return Array.isArray(values) ? values.flat(1) : null; }
    case "objectValues": { const value = query(node.expression, input); return record(value) ? Object.values(value) : null; }
    case "slice": {
      if (!Array.isArray(input)) return null;
      const length = input.length, step = node.step; if (!step) throw new TypeError("JMESPath slice step cannot be zero");
      const bound = (value: number | null, fallback: number): number => value === null ? fallback : Math.max(step < 0 ? -1 : 0, Math.min(step < 0 ? length - 1 : length, value < 0 ? length + value : value));
      const start = bound(node.start, step < 0 ? length - 1 : 0), stop = bound(node.stop, step < 0 ? -1 : length), output = [];
      for (let index = start; step < 0 ? index > stop : index < stop; index += step) output.push(input[index]); return output;
    }
    case "multiList": return input == null ? null : node.expressions.map(node => query(node, input));
    case "multiHash": return input == null ? null : Object.fromEntries(node.entries.map(entry => [entry.key, query(entry.value, input)]));
    case "and": { const value = query(node.left, input); return truth(value) ? query(node.right, input) : value; }
    case "or": { const value = query(node.left, input); return truth(value) ? value : query(node.right, input); }
    case "not": return !truth(query(node.expression, input));
    case "condition": return truth(query(node.predicate, input)) ? query(node.then, input) : null;
    case "expressionReference": return new Reference(node.expression);
    case "function": return invoke(node.function, node.arguments.map(node => query(node, input)));
    case "compare": {
      const left = query(node.left, input), right = query(node.right, input);
      if (node.comparator === "equal") return equal(left, right);
      if (node.comparator === "notEqual") return !equal(left, right);
      if (numericText(left) === undefined || numericText(right) === undefined) return null;
      const order = compare(left, right);
      switch (node.comparator) { case "lessThan": return order < 0; case "lessThanEqual": return order <= 0; case "greaterThan": return order > 0; case "greaterThanEqual": return order >= 0; default: throw new TypeError("unknown JMESPath comparator"); }
    }
  }
}
