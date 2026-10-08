// Exercises the generated runtime validation directly; run from the package root after `npm run build`.
const assert = require('node:assert/strict');
const { toWire, validate } = require('./lib/validation');

const none = { minLength: null, maxLength: null, min: null, max: null, pattern: null, unique: false };
const shapes = {
  'x#Int': { kind: 'number', integer: true, intRange: [-2147483648, 2147483647], constraints: none },
  'x#Byte': { kind: 'number', integer: true, intRange: [-128, 127], constraints: none },
  'x#Str': { kind: 'string', constraints: none },
  'x#Item': { kind: 'object', members: { max_count: { wire: 'max_count', target: 'x#Int', required: true, constraints: none } } },
  'x#Outer': { kind: 'object', members: {
    maxCount: { wire: 'Max_Count', target: 'x#Int', required: false, constraints: none },
    items: { wire: 'Items', target: 'x#Items', required: false, constraints: none },
  } },
  'x#Items': { kind: 'array', element: { wire: 'member', target: 'x#Item', constraints: none }, constraints: none },
};
const fails = (value, target, pattern) => assert.throws(() => validate(value, { target, constraints: none }, shapes, 'p'), pattern);
const ok = (value, target) => validate(value, { target, constraints: none }, shapes, 'p');

// An Integer without @range still has to fit 32 bits; Byte has to fit 8.
ok(2147483647, 'x#Int');
fails(3000000000, 'x#Int', /outside integer type range/);
fails(-2147483649, 'x#Int', /outside integer type range/);
fails(128, 'x#Byte', /outside integer type range/);
fails(1.5, 'x#Int', /safe integer/);

// Valid ECMA-262 patterns must not be rejected for the unicode flag.
const pattern = (source, value) => validate(value, { target: 'x#Str', constraints: { ...none, pattern: source } }, shapes, 'p');
pattern(String.raw`^a\-b$`, 'a-b');
pattern(String.raw`^[\w-.]+$`, 'a-b.c');
assert.throws(() => pattern(String.raw`^a\-b$`, 'ab'), /pattern mismatch/);
pattern('^.$', '\u{1F600}');
assert.throws(() => pattern('^.$', 'ab'), /pattern mismatch/);

// jsii-spelled members go to the provider under their wire names.
assert.deepEqual(
  toWire({ maxCount: 1, items: [{ max_count: 2 }] }, 'x#Outer', shapes),
  { Max_Count: 1, Items: [{ max_count: 2 }] },
);
console.log('CDK runtime validation passed');
