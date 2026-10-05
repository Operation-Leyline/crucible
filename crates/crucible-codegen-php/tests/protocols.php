<?php
declare(strict_types=1);
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'ProtocolSdk\\')) require getenv('PHP_PROTOCOL_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 12)) . '.php';
});
use ProtocolSdk\{ChoiceText, JsonServiceClient, ProblemFault, ValuesInput};
use ProtocolSdk\Runtime\{Absent, CallScope, ClientConfig, HttpRequest, HttpResponse, HttpTransport, JsonValue, Timestamp, Unit, UnknownServiceError};
error_reporting(E_ALL);
set_error_handler(static function(int $severity, string $message, string $file, int $line): never { throw new ErrorException($message, 0, $severity, $file, $line); });
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException($message); }
$protocol = getenv('PROTOCOL');
$version = ['awsJson1_0' => '1.0', 'awsJson1_1' => '1.1'][$protocol] ?? null;
final class Server implements HttpTransport {
    /** @var list<HttpRequest> */
    public array $requests = [];
    public function __construct(private Closure $respond) {}
    public function send(HttpRequest $request, bool $readBody, CallScope $scope): HttpResponse { $this->requests[] = $request; return ($this->respond)($request); }
    public function close(): void {}
}
$client = static fn(Server $server) => new JsonServiceClient(new ClientConfig(endpoint: 'https://example.test', transport: $server));
$server = new Server(static function(HttpRequest $request) use ($version): HttpResponse {
    check($request->method === 'POST', 'method');
    $body = json_decode($request->body, true, flags: JSON_THROW_ON_ERROR);
    check($body['when'] === -0.25 && $body['iso'] === '1969-12-31T23:59:59.75Z', 'body timestamps');
    check(!array_key_exists('nullable', $body) && array_key_exists('document', $body) && $body['document'] === null, 'structure null omitted, document null kept');
    check($body['floatValue'] === 'NaN' && $body['doubleValue'] === 'Infinity', 'non-finite floats');
    check($body['choice'] === ['text' => 'hi'], 'union');
    if ($version !== null) {
        check(parse_url($request->url, PHP_URL_PATH) === '/' && parse_url($request->url, PHP_URL_QUERY) === null, 'RPC path');
        check($request->headers['x-amz-target'] === 'JsonService.Echo' && $request->headers['content-type'] === 'application/x-amz-json-' . $version, 'RPC headers');
        check($body['renamed'] === 'hello' && $body['date'] === -0.25 && $body['dates'] === [-0.25, 1] && $body['query'] === -0.25, 'RPC body members');
        check(!isset($request->headers['x-date']), 'RPC has no header bindings');
    } else {
        check(parse_url($request->url, PHP_URL_PATH) === '/echo', 'REST path');
        check(parse_url($request->url, PHP_URL_QUERY) === 'time=1969-12-31T23%3A59%3A59.75Z&times=1969-12-31T23%3A59%3A59.75Z&times=1970-01-01T00%3A00%3A01Z', 'REST query timestamps');
        check($request->headers['x-date'] === 'Wed, 31 Dec 1969 23:59:59 GMT' && $request->headers['x-dates'] === '"Wed, 31 Dec 1969 23:59:59 GMT", "Thu, 01 Jan 1970 00:00:01 GMT"', 'REST header timestamps');
        check($request->headers['content-type'] === 'application/json' && $body['wire-name'] === 'hello' && !isset($body['date']) && !isset($body['query']), 'REST body');
    }
    $body['choice'] = ['text' => 'ok', '__type' => 'ignored'];
    $body['nullable'] = null;
    $headers = $version === null ? ['x-date' => 'Thu, 01 Jan 1970 00:00:01 GMT', 'x-dates' => '"Wed, 31 Dec 1969 23:59:59 GMT", "Thu, 01 Jan 1970 00:00:01 GMT"'] : [];
    return new HttpResponse(200, $headers, json_encode($body, JSON_THROW_ON_ERROR));
});
$instant = new Timestamp(-1, 750000000);
$output = $client($server)->echoOperation(new ValuesInput(renamed: 'hello', when: $instant, iso: $instant, date: $instant, dates: [$instant, new Timestamp(1)],
    query: $instant, queries: [$instant, new Timestamp(1)], choice: new ChoiceText('hi'), floatValue: NAN, doubleValue: INF, nullable: null, document: null));
check(count($server->requests) === 1, 'one request');
check($output->choice instanceof ChoiceText && $output->choice->value === 'ok', 'unknown __type ignored in union');
check($output->nullable === Absent::Value && $output->document === null, 'response nulls');
check(is_nan($output->floatValue) && $output->doubleValue === INF, 'response non-finite floats');
check($output->when == $instant && $output->iso == $instant && $output->renamed === 'hello', 'response body values');
if ($version === null) check($output->date == new Timestamp(1) && $output->dates == [new Timestamp(-1), new Timestamp(1)], 'response header timestamps');
else check($output->date == $instant && $output->dates == [$instant, new Timestamp(1)], 'response body timestamps');
$defaults = $client(new Server(static fn() => new HttpResponse(200, [], '{}')))->echoOperation(new ValuesInput());
check($defaults->when == $instant && $defaults->iso == $instant, 'modeled defaults');

// AWS error discriminators: header, code, and namespaced __type, with AWS request IDs.
foreach ([[['x-amzn-errortype' => 'example.json#Problem:http://internal'], ['__type' => 'Other', 'Message' => 'bad']],
          [[], ['code' => 'Problem', 'Message' => 'bad']], [[], ['__type' => 'example.json#Problem', 'message' => 'bad']]] as [$headers, $body]) {
    $server = new Server(static fn() => new HttpResponse(409, ['x-amzn-requestid' => 'request-42', ...$headers], json_encode($body)));
    try { $client($server)->echoOperation(new ValuesInput()); throw new RuntimeException('expected modeled error'); }
    catch (ProblemFault $error) {
        check($error->metadata->name === 'Problem' && $error->metadata->requestId === 'request-42' && $error->metadata->message === 'bad' && $error->getMessage() === 'bad', 'modeled AWS error');
        check($error->metadata->status === 409 && $error->messageValue === ($body['message'] ?? Absent::Value), 'modeled members');
    }
}
$server = new Server(static fn() => new HttpResponse(500, ['x-amz-request-id' => 'fallback'], '{"__type":"aws.example#Unmodeled"}'));
try { $client($server)->echoOperation(new ValuesInput()); throw new RuntimeException('expected unknown error'); }
catch (UnknownServiceError $error) { check($error->metadata->name === 'Unmodeled' && $error->metadata->requestId === 'fallback', 'unknown AWS error'); }

// An operation without input still sends the RPC envelope; REST sends no body.
$server = new Server(static function(HttpRequest $request) use ($version): HttpResponse {
    check($request->body === ($version === null ? null : '{}'), 'empty operation body');
    if ($version !== null) check($request->headers['x-amz-target'] === 'JsonService.Empty', 'empty operation target');
    else check(parse_url($request->url, PHP_URL_PATH) === '/empty', 'empty REST path');
    return new HttpResponse(200);
});
check($client($server)->emptyOperation() instanceof Unit, 'empty operation');
echo "PHP $protocol protocol passed\n";
