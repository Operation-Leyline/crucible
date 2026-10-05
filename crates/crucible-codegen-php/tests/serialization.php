<?php
declare(strict_types=1);
spl_autoload_register(function (string $class): void {
    require getenv('PHP_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, strlen('Example\\Weather\\'))) . '.php';
});
use Example\Weather\Schema;
use Example\Weather\Runtime\{ExactJson, Field, JsonNumber, JsonObject};

function assertSameWire(mixed $actual, mixed $expected): void {
    if ($expected instanceof JsonNumber) {
        if (!$actual instanceof JsonNumber || $actual->scaled(9, 64) !== $expected->scaled(9, 64)) throw new RuntimeException('Exact numeric mismatch');
    } elseif ($expected instanceof JsonObject) {
        if (!$actual instanceof JsonObject || count($actual->members) !== count($expected->members)) throw new RuntimeException('Object mismatch');
        foreach ($expected->members as $key => $value) {
            if (!$actual->has((string)$key)) throw new RuntimeException('Missing object key');
            assertSameWire($actual->get((string)$key), $value);
        }
    } elseif (is_array($expected)) {
        if (!is_array($actual) || count($actual) !== count($expected)) throw new RuntimeException('Array mismatch');
        foreach ($expected as $index => $value) assertSameWire($actual[$index], $value);
    } elseif ($actual !== $expected) throw new RuntimeException('Scalar mismatch');
}
$codec = Schema::codec();
$cases = json_decode(file_get_contents(__DIR__ . '/../../../tests/conformance/serialization.json'), true, flags: JSON_THROW_ON_ERROR);
foreach ($cases as $row) {
    try {
        $context = $codec->context($row['target']);
        try { $native = $codec->decode(ExactJson::decode($row['wire']), $context); }
        catch (UnexpectedValueException|InvalidArgumentException|TypeError $error) {
            if ($row['reject'] ?? false) continue;
            throw $error;
        }
        if ($row['reject'] ?? false) throw new RuntimeException('Expected rejection');
        $expected = $row['expect'];
        assertSameWire(ExactJson::decode(ExactJson::encode($codec->encode($native, $context))), ExactJson::decode($expected['json']));
        foreach ($expected['states'] ?? [] as $member => $state) {
            $value = $native->$member;
            $actual = $value instanceof Field ? (!$value->isSet ? 'absent' : ($value->value === null ? 'null' : 'value')) : 'value';
            if ($actual !== $state) throw new RuntimeException('Native presence mismatch: ' . $member);
        }
        if (isset($expected['blobHex']) && bin2hex($native) !== $expected['blobHex']) throw new RuntimeException('Blob mismatch');
        if (isset($expected['instant'])) {
            $instant = $expected['instant'];
            $stamp = $native->{$instant['member']}->value;
            if ((string)$stamp->seconds !== $instant['seconds'] || $stamp->nanoseconds !== $instant['nanoseconds']) throw new RuntimeException('Native instant mismatch');
        }
    } catch (Throwable $error) { throw new RuntimeException($row['id'] . ': ' . $error->getMessage(), previous: $error); }
}
echo 'PHP shared serialization: ' . count($cases) . " cases passed\n";
