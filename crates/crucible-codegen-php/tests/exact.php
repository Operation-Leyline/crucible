<?php
declare(strict_types=1);
use CrucibleGenerated\Runtime\{ExactJson, JsonNumber, JsonObject, JsonValue, Timestamp};
spl_autoload_register(function (string $class): void {
    $name = substr($class, strrpos($class, '\\') + 1);
    require __DIR__ . '/../runtime/' . $name . '.php';
});
function check(bool $value, string $message): void { if (!$value) throw new RuntimeException($message); }
function rejects(callable $action): void {
    try { $action(); } catch (UnexpectedValueException) { return; }
    throw new RuntimeException('Expected invalid value');
}
foreach (['-9223372036854775808', '9223372036854775807', '9007199254740993'] as $text) {
    $number = ExactJson::decode($text);
    check((string)$number->integer() === $text, 'Exact integer');
    check(ExactJson::encode($number) === $text, 'Exact JSON');
}
foreach (['9223372036854775808', '-9223372036854775809', '1.2', '1e99999999'] as $text) rejects(fn() => (new JsonNumber($text))->integer());
check((new JsonNumber('12300e-2'))->integer() === 123, 'Integral exponent');
check((new JsonNumber('-0e999999'))->integer() === 0, 'Scaled zero');
foreach (['', '01', '1.', '[1,]', '{"x":1,}', 'true false', '"\\ud800"', '"a' . "\x01" . '"'] as $source) rejects(fn() => ExactJson::decode($source));
$object = ExactJson::decode('{"0":1,"00":2,"":3,"a":{},"b":[]}');
check(ExactJson::encode($object) === '{"":3,"0":1,"00":2,"a":{},"b":[]}', 'Object key and empty collection identities');
$members = ['nested' => new JsonObject(['original' => true])];
$document = new JsonValue(new JsonObject($members));
$members['changed'] = true;
check(!str_contains($document->toJson(), 'changed'), 'Document ownership');
check(ExactJson::encode(ExactJson::decode('{"\\u0000a":1}')) === '{"\\u0000a":1}', 'NUL object key');
foreach (['-9223372036854775808', '9223372036854775807.999999999', '-0.000000001', '-9223372036854775807.999999999', '0', '1.123456789'] as $text) {
    check(Timestamp::fromEpoch(new JsonNumber($text))->toEpoch()->text === $text, 'Exact epoch ' . $text);
}
foreach (['0001-01-01T00:00:00Z', '9999-12-31T23:59:59.999999999Z', '1969-12-31T23:59:59.999999999Z', '2024-02-29T12:00:00.123456789Z'] as $text) {
    check(Timestamp::parse($text)->toDateTimeString() === $text, 'Calendar round trip');
}
check(Timestamp::parse('1970-01-01T01:00:00+01:00')->seconds === 0, 'Timezone offset');
foreach (['0000-01-01T00:00:00Z', '2023-02-29T00:00:00Z', '2024-01-01T24:00:00Z', '2024-01-01T00:00:00+24:00'] as $text) rejects(fn() => Timestamp::parse($text));
rejects(fn() => Timestamp::fromEpoch(new JsonNumber('-9223372036854775808.1')));
rejects(fn() => (new Timestamp(PHP_INT_MAX))->toDateTimeString());
rejects(fn() => Timestamp::fromHttpDate('Mon, 01 Jan 1970 00:00:00 GMT'));
check(Timestamp::fromHttpDate('Thu, 01 Jan 1970 00:00:00 GMT')->toHttpDate() === 'Thu, 01 Jan 1970 00:00:00 GMT', 'HTTP date');
check(Timestamp::fromDateTime((new Timestamp(-1, 999999999))->toDateTime())->toEpoch()->text === '-0.000001', 'Microsecond conversion truncates');
rejects(fn() => ExactJson::decode(str_repeat('[', 257) . '0' . str_repeat(']', 257)));
echo "PHP exact JSON and timestamp checks passed\n";
