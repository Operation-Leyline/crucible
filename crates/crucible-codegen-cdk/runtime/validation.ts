import { Token } from 'aws-cdk-lib';

// This module is deliberately absent from the public jsii entry point.
// Diagnostics contain paths and rules, never supplied property values.
export function validate(value: unknown, schema: any, shapes: any, path: string, required = true, depth = 0): void {
  const fail = (rule: string): never => { throw new Error(`${path}: ${rule}`); };
  if (value === undefined) {
    if (required) fail('required property is missing');
    return;
  }
  if (Token.isUnresolved(value)) return;
  if (value === null) fail('null is not supported');
  if (depth > 128) fail('value exceeds maximum nesting depth');
  if (schema.target) {
    const target = shapes[schema.target];
    validate(value, { ...target, constraints: schema.constraints ?? target.constraints,
      integer: schema.integer || target.integer, enumValues: schema.enumValues ?? target.enumValues,
      timestamp: schema.timestamp ?? target.timestamp }, shapes, path, true, depth + 1);
    return;
  }
  const kind = schema.kind;
  if (kind === 'string' && typeof value !== 'string') fail('expected string');
  if (kind === 'number' && (typeof value !== 'number' || !Number.isFinite(value))) fail('expected finite number');
  if (kind === 'boolean' && typeof value !== 'boolean') fail('expected boolean');
  if (kind === 'timestamp') {
    if (schema.timestamp === 'epoch-seconds') {
      if (typeof value !== 'number' || !Number.isFinite(value)) fail('expected epoch seconds');
    } else if (typeof value !== 'string' || Number.isNaN(Date.parse(value))) {
      fail('expected timestamp string');
    } else if (schema.timestamp === 'date-time' && !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?(?:Z|[+-]\d\d:\d\d)$/.test(value)) {
      fail('expected RFC 3339 timestamp');
    } else if (schema.timestamp === 'http-date' && !/^(Mon|Tue|Wed|Thu|Fri|Sat|Sun), \d\d (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) \d{4} \d\d:\d\d:\d\d GMT$/.test(value)) {
      fail('expected HTTP date timestamp');
    }
  }
  if (schema.integer && !Number.isSafeInteger(value)) fail('expected safe integer');
  if (schema.enumValues && !schema.enumValues.includes(value)) fail('invalid enum value');
  if (kind === 'array') {
    if (!Array.isArray(value)) fail('expected array');
    (value as unknown[]).forEach((item, i) => validate(item, schema.element, shapes, `${path}[${i}]`, true, depth + 1));
  }
  if (kind === 'object' || kind === 'map') {
    if (typeof value !== 'object' || Array.isArray(value)) fail('expected object');
    const record = value as Record<string, unknown>;
    if (kind === 'object') {
      for (const [key, member] of Object.entries<any>(schema.members)) {
        validate(record[key], member, shapes, `${path}.${key}`, member.required, depth + 1);
      }
    } else {
      for (const key of Object.keys(record)) {
        validate(key, schema.key, shapes, `${path}.<key>`, true, depth + 1);
        validate(record[key], schema.element, shapes, `${path}.<value>`, true, depth + 1);
      }
    }
  }
  const c = schema.constraints;
  if (!c) return;
  const length = typeof value === 'string' ? [...value].length : Array.isArray(value) ? value.length : typeof value === 'object' ? Object.keys(value as object).length : undefined;
  if (c.minLength != null && length !== undefined && length < c.minLength) fail('below minimum length');
  if (c.maxLength != null && length !== undefined && length > c.maxLength) fail('above maximum length');
  if (c.min != null && typeof value === 'number' && value < c.min) fail('below minimum');
  if (c.max != null && typeof value === 'number' && value > c.max) fail('above maximum');
  if (c.pattern != null && typeof value === 'string' && !new RegExp(c.pattern, 'u').test(value)) fail('pattern mismatch');
  // Token values can coincide before deployment, so defer uniqueness when any occur.
  if (c.unique && Array.isArray(value) && !containsToken(value)) {
    const normalized = value.map(canonical);
    if (new Set(normalized).size !== value.length) fail('duplicate item');
  }
}

function containsToken(value: unknown): boolean {
  if (Token.isUnresolved(value)) return true;
  if (Array.isArray(value)) return value.some(containsToken);
  return value !== null && typeof value === 'object' && Object.values(value).some(containsToken);
}

function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value !== null && typeof value === 'object') {
    const record = value as Record<string, unknown>;
    return `{${Object.keys(record).sort().map(key => `${JSON.stringify(key)}:${canonical(record[key])}`).join(',')}}`;
  }
  return JSON.stringify(value);
}
