<?php
declare(strict_types=1);
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'Types\\')) require getenv('PHP_EXTENDED_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 6)) . '.php';
});
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException($message); }
function rejects(callable $call): void { try { $call(); } catch (UnexpectedValueException) { return; } throw new RuntimeException('Accepted invalid value'); }
$codec = Types\Schema::codec();
$node = Types\Runtime\ExactJson::decode('{"mode":123,"names":["a","b"],"sparseNames":[null,"x"],"sparseMap":{"absent":null,"future":123}}');
$output = $codec->decode($node, $codec->context('example.types#ReadOutput'));
check((string)$output->integer === '12345678901234567890123456789012345678901234567890', 'integer');
check((string)$output->decimal === '12345678901234567890.12345678901234567890123456789', 'decimal');
check($output->sparseNames === [null, 'x'] && array_key_exists('absent', $output->sparseMap) && $output->sparseMap['absent'] === null, 'sparse');
check($output->mode->value === 123 && $output->sparseMap['future']->value === 123, 'future enum');
$encoded = $codec->encode($output, $codec->context('example.types#ReadOutput'));
check($codec->equivalent($encoded->get('sparseMap'), $node->get('sparseMap')), 'roundtrip');
rejects(fn() => $codec->decode(['a', 'a'], $codec->context('example.types#Names')));
rejects(fn() => $codec->decode([null], $codec->context('example.types#Names')));
rejects(fn() => $codec->decode(new Types\Runtime\JsonNumber('2147483648'), $codec->context('example.types#Mode')));
rejects(fn() => new Types\Runtime\BigInteger('1.000000000000000000001'));
check((new Types\Runtime\BigDecimal('1e999999999999999999999999'))->equals(new Types\Runtime\BigDecimal('10e999999999999999999999998')), 'exact equality');
check(!(new Types\Runtime\BigDecimal('1.000000000000000000001'))->equals(new Types\Runtime\BigDecimal('1')), 'exact inequality');
foreach (['Integers' => '[18446744073709551616,18446744073709551617]', 'Decimals' => '[1.00000000000000000001,1.00000000000000000002]'] as $shape => $json) {
    $codec->decode(Types\Runtime\ExactJson::decode($json), $codec->context('example.types#' . $shape));
    rejects(fn() => $codec->decode(Types\Runtime\ExactJson::decode('[1,1.0]'), $codec->context('example.types#' . $shape)));
}
echo "PHP exact extended types passed\n";
