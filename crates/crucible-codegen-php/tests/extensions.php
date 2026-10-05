<?php
declare(strict_types=1);
spl_autoload_register(static function(string $class): void {
    foreach (['Dynamic' => getenv('PHP_DYNAMIC_PACKAGE'), 'Types' => getenv('PHP_EXTENDED_PACKAGE')] as $namespace => $path) {
        if (str_starts_with($class, $namespace . '\\')) require $path . '/src/' . str_replace('\\', '/', substr($class, strlen($namespace) + 1)) . '.php';
    }
});
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException($message); }
function rejects(callable $call): void { try { $call(); } catch (UnexpectedValueException) { return; } throw new RuntimeException('Accepted invalid value'); }
final class Fake implements Dynamic\Runtime\HttpTransport {
    public function __construct(private Dynamic\Runtime\HttpResponse $response) {}
    public function send(Dynamic\Runtime\HttpRequest $request, bool $readBody, Dynamic\Runtime\CallScope $scope): Dynamic\Runtime\HttpResponse {
        check($request->method === 'POST', 'method');
        check($request->url === 'http://127.0.0.1/files/folder/a%20b?empty=&tag=hello%20world&tag=%2F', 'URI');
        check($request->headers['x-meta-trace'] === 'abc', 'prefix header');
        check(json_decode($request->body, true) === ['wire-value' => 'hello'], 'json name');
        return $this->response;
    }
    public function close(): void {}
}
$input = new Dynamic\Input(key: 'folder/a b', query: ['tag' => ['hello world', '/'], 'empty' => ['']], metadataValue: ['trace' => 'abc'], value: 'hello');
$client = new Dynamic\DynamicClient(new Dynamic\Runtime\ClientConfig(endpoint: 'http://127.0.0.1', transport: new Fake(new Dynamic\Runtime\HttpResponse(206, ['X-Meta-Trace' => 'abc'], '{"wire-value":"result","value":"ignored"}'))));
$output = $client->get($input);
check($output->status === 206 && $output->metadataValue === ['trace' => 'abc'] && $output->value === 'result', 'response bindings');
$client = new Dynamic\DynamicClient(new Dynamic\Runtime\ClientConfig(endpoint: 'http://127.0.0.1', transport: new Fake(new Dynamic\Runtime\HttpResponse(422, ['x-crucible-error' => 'Problem', 'x-meta-reason' => 'invalid'], '{}'))));
try { $client->get($input); throw new RuntimeException('Expected modeled error'); }
catch (Dynamic\Problem $error) { check($error->status === 422 && $error->details === ['reason' => 'invalid'], 'error bindings'); }
foreach (['a/../b', './a', 'a/.'] as $key) rejects(fn() => $client->get(new Dynamic\Input(key: $key)));
$codec = Types\Schema::codec();
$node = Types\Runtime\ExactJson::decode('{"mode":123,"names":["a","b"],"sparseNames":[null,"x"],"sparseMap":{"absent":null,"future":123}}');
$output = $codec->decode($node, $codec->context('example.types#Output'));
check((string)$output->integer === '12345678901234567890123456789012345678901234567890', 'integer');
check((string)$output->decimal === '12345678901234567890.12345678901234567890123456789', 'decimal');
check($output->sparseNames === [null, 'x'] && array_key_exists('absent', $output->sparseMap) && $output->sparseMap['absent'] === null, 'sparse');
check($output->mode->value === 123 && $output->sparseMap['future']->value === 123, 'future enum');
$encoded = $codec->encode($output, $codec->context('example.types#Output'));
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
echo "PHP dynamic bindings and exact extended types passed\n";
