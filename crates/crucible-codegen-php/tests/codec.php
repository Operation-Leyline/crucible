<?php
declare(strict_types=1);
require __DIR__ . '/exact.php';
use CrucibleGenerated\Runtime\{ExactJson, WireCodec, JsonValue, JsonObject, JsonNumber};
$schema = WireCodec::object(ExactJson::decode('{"shapes":{
  "doc":{"kind":"document"}, "long":{"kind":"long"}, "str":{"kind":"string"},
  "list":{"kind":"list","member":{"target":"str"}},
  "time":{"kind":"timestamp","format":"date-time"},
  "dates":{"kind":"list","member":{"target":"time","format":"http-date"}},
  "parent":{"kind":"structure","members":{
    "doc":{"target":"doc","default":null},
    "count":{"target":"long","default":9223372036854775807},
    "optional":{"target":"long","default":2}
  }}
}}'));
$codec = new WireCodec($schema, fn() => throw new RuntimeException('Unexpected model'));
$context = $codec->context('parent');
$null = $context->read('doc', new JsonObject(['doc' => null]));
check($null instanceof JsonValue && $null->toJson() === 'null', 'Required document null');
rejects(fn() => $context->read('doc', new JsonObject()));
check($context->read('count',new JsonObject()) === PHP_INT_MAX,'Exact numeric default');
rejects(fn() => $context->read('count',new JsonObject(['count' => null])));
check($context->optional('optional',new JsonObject()) === 2,'Optional default');
check($context->optional('optional',new JsonObject(['optional' => null])) === null,'Explicit null suppresses default');
$cases = json_decode(file_get_contents(__DIR__ . '/../../../tests/conformance/header-lists.json'), true, flags:JSON_THROW_ON_ERROR);
foreach ($cases as $case) {
    $member = new JsonObject(['target' => 'list']);
    check($codec->header($member,$case['wire']) === $case['values'], 'Shared header parse');
    check($codec->header($member,$codec->bound($case['values'])) === $case['values'],'Shared header round trip');
}
$dates = ['Wed, 31 Dec 1969 23:59:59 GMT', 'Thu, 01 Jan 1970 00:00:00 GMT'];
foreach ([implode(', ', $dates), $codec->bound($dates)] as $wire) {
    check($codec->header(new JsonObject(['target' => 'dates']), $wire) === $dates, 'HTTP-date header lists preserve date commas');
    check($codec->header(new JsonObject(['target' => 'dates']), $wire, 'http-date') === $dates, 'HTTP-date header list binding override');
}
echo "PHP schema codec checks passed\n";
