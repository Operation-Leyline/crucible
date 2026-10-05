<?php
declare(strict_types=1);
// Runs the official Smithy RPC v2 protocol tests (lowered into schema.json) against the generated client.
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'RpcSdk\\')) require getenv('PHP_RPC_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 7)) . '.php';
});
use RpcSdk\{Decoder, Schema};
use RpcSdk\Runtime\{CallScope, Cbor, CborBytes, CborTag, ClientConfig, ClientRuntime, ExactJson, HttpRequest, HttpResponse, HttpTransport, JsonNumber, JsonObject, ServiceError, Timestamp, Unit, WireCodec};
error_reporting(E_ALL);
set_error_handler(static function(int $severity, string $message, string $file, int $line): never { throw new ErrorException($message, 0, $severity, $file, $line); });

$protocol = getenv('PROTOCOL');
$cbor = $protocol === 'rpcv2Cbor';
final class Recorder implements HttpTransport {
    public ?HttpRequest $request = null;
    public function __construct(public HttpResponse $response) {}
    public function send(HttpRequest $request, bool $readBody, CallScope $scope): HttpResponse { $this->request = $request; return $this->response; }
    public function close(): void {}
}
$codec = Schema::codec();
$bigStrings = $codec->schema->get('protocolSettings')?->get('arbitraryPrecisionStrings') === true;

/** Smithy test params (node values) to the wire values WireCodec decodes. */
$node = static function(string $target, mixed $value) use (&$node, $codec, $bigStrings): mixed {
    if ($value === null) return null;
    $shape = $codec->shape($target);
    switch ($shape->get('kind')) {
        case 'structure': case 'union':
            $members = WireCodec::object($shape->get('members')); $result = [];
            foreach (WireCodec::object($value)->members as $name => $item) $result[$name] = $node(WireCodec::object($members->get((string)$name))->get('target'), $item);
            return new JsonObject($result);
        case 'list': return array_map(fn($item) => $node($shape->get('member')->get('target'), $item), $value);
        case 'map': return new JsonObject(array_map(fn($item) => $node($shape->get('value')->get('target'), $item), WireCodec::object($value)->members));
        case 'blob': return base64_encode($value);
        case 'timestamp': return is_string($value) ? Timestamp::parse($value)->toEpoch() : $value;
        case 'bigInteger': case 'bigDecimal': return $bigStrings ? $value->text : $value;
        default: return $value;
    }
};
function same(mixed $left, mixed $right): bool {
    if ((is_int($left) || is_float($left)) && (is_int($right) || is_float($right))) {
        if (is_float($left) && is_nan($left)) return is_float($right) && is_nan($right);
        return (float)$left === (float)$right;
    }
    if ($left instanceof JsonNumber && $right instanceof JsonNumber) return $left->canonical() === $right->canonical();
    if ($left instanceof CborBytes && $right instanceof CborBytes) return $left->bytes === $right->bytes;
    if ($left instanceof CborTag && $right instanceof CborTag) return $left->tag === $right->tag && same($left->value, $right->value);
    if ($left instanceof JsonObject && $right instanceof JsonObject) {
        if (count($left->members) !== count($right->members)) return false;
        foreach ($left->members as $key => $value) if (!$right->has((string)$key) || !same($value, $right->get((string)$key))) return false;
        return true;
    }
    if (is_array($left) && is_array($right)) {
        if (count($left) !== count($right)) return false;
        foreach ($left as $i => $value) if (!array_key_exists($i, $right) || !same($value, $right[$i])) return false;
        return true;
    }
    return $left === $right;
}
function show(mixed $value): string {
    return match (true) {
        $value instanceof CborBytes => "h'" . bin2hex($value->bytes) . "'",
        $value instanceof CborTag => $value->tag . '(' . show($value->value) . ')',
        $value instanceof JsonObject => '{' . implode(', ', array_map(fn($k, $v) => "$k: " . show($v), array_keys($value->members), $value->members)) . '}',
        is_array($value) => '[' . implode(', ', array_map('show', $value)) . ']',
        $value instanceof JsonNumber => $value->text,
        is_float($value) => var_export($value, true),
        default => json_encode($value),
    };
}
$headers = static fn(JsonObject $message): array => array_change_key_case(WireCodec::object($message->get('headers'))->members, CASE_LOWER);

$ran = 0; $skipped = [];
foreach (WireCodec::items($codec->schema->get('protocolTests')) as $test) {
    $id = $test->get('id');
    if ($test->get('protocol') !== 'smithy.protocols#' . $protocol || $test->get('appliesTo') === 'server') continue;
    $case = $test->get('case'); $message = $case->get('message');
    $operation = $test->get('operation'); $name = substr($operation, strpos($operation, '#') + 1);
    $op = WireCodec::object(WireCodec::object($codec->schema->get('operations'))->get($name));
    $fail = static function(string $why) use ($id): never { throw new RuntimeException("$id: $why"); };
    if ($case->get('kind') === 'request') {
        $transport = new Recorder(new HttpResponse(500, [], ''));
        $runtime = new ClientRuntime(new ClientConfig(endpoint: 'https://example.test', transport: $transport, maxAttempts: 1), $codec, Decoder::error(...), '', false);
        $input = $test->get('shape') === null ? new Unit() : $codec->decode($node($test->get('shape'), $test->get('params')), $codec->context($test->get('shape')));
        try { $runtime->call($name, $input); } catch (ServiceError) { /* Only the request is under test. */ }
        $request = $transport->request ?? $fail('no request was sent');
        if ($request->method !== $case->get('method')) $fail('method ' . $request->method);
        $url = parse_url($request->url);
        if (($url['path'] ?? '/') !== $case->get('uri') || isset($url['query'])) $fail('URI ' . $request->url);
        $sent = array_change_key_case($request->headers, CASE_LOWER);
        foreach ($headers($message) as $header => $value) if (($sent[$header] ?? null) !== $value) $fail("header $header: " . ($sent[$header] ?? 'missing'));
        foreach (WireCodec::items($message->get('forbidHeaders')) as $header) if (isset($sent[strtolower($header)])) $fail("forbidden header $header");
        foreach (WireCodec::items($message->get('requireHeaders')) as $header) {
            // cURL derives Content-Length from a buffered body; the loopback test observes it on the wire.
            if (strtolower($header) === 'content-length' ? $request->body === null : !isset($sent[strtolower($header)])) $fail("required header $header");
        }
        $expected = $message->get('body');
        if ($expected === null || $expected === '') {
            if (($request->body ?? '') !== '') $fail('unexpected body');
        } else {
            $actualBody = $request->body ?? $fail('missing body');
            [$want, $got] = $cbor ? [Cbor::decode(base64_decode($expected, true)), Cbor::decode($actualBody)] : [ExactJson::decode($expected), ExactJson::decode($actualBody)];
            if (!same($want, $got)) $fail("body\n  want " . show($want) . "\n  got  " . show($got));
        }
    } else {
        $body = $message->get('body') ?? '';
        if ($cbor) $body = base64_decode($body, true);
        $transport = new Recorder(new HttpResponse($case->get('code')->integer(), $headers($message), $body));
        $runtime = new ClientRuntime(new ClientConfig(endpoint: 'https://example.test', transport: $transport, maxAttempts: 1), $codec, Decoder::error(...), '', false);
        $input = $op->get('input') === null ? new Unit() : $codec->decode(new JsonObject(), $codec->context($op->get('input')));
        $shape = $test->get('shape');
        try {
            $output = $runtime->call($name, $input);
            if ($test->get('error')) $fail('expected an error');
        } catch (ServiceError $error) {
            if (!$test->get('error')) $fail('unexpected error ' . $error->metadata->name . ': ' . $error->getMessage());
            $class = 'RpcSdk\\' . $codec->shape($shape)->get('name');
            if (!$error instanceof $class) $fail('error class ' . $error::class);
            $output = $error;
        }
        if ($shape !== null && $test->get('params') !== null) {
            $want = $node($shape, $test->get('params'));
            $got = $codec->encode($output, $codec->context($shape));
            if (!$codec->equivalent($want, $got)) $fail("output\n  want " . show($want) . "\n  got  " . show($got));
        }
    }
    $ran++;
}
if ($ran < 40) throw new RuntimeException("only $ran protocol tests ran");
echo "PHP $protocol: $ran official Smithy protocol tests passed\n";
