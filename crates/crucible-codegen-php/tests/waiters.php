<?php
declare(strict_types=1);
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'JobsSdk\\')) require getenv('PHP_JOBS_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 8)) . '.php';
});
use JobsSdk\{Input, JobsClient};
use JobsSdk\Runtime\{CallScope, CancellationToken, CancelledException, ClientConfig, ExactJson, HttpRequest, HttpResponse, HttpTransport, JsonNumber, JsonObject, Query, QueryException, ServiceError, Waiter, WaiterException, WaiterTimeoutException};
error_reporting(E_ALL);
set_error_handler(static function(int $severity, string $message, string $file, int $line): never { throw new ErrorException($message, 0, $severity, $file, $line); });
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException($message); }
final class Script implements HttpTransport {
    public int $calls = 0;
    /** @var list<HttpRequest> */
    public array $requests = [];
    public function __construct(public Closure $respond) {}
    public function send(HttpRequest $request, bool $readBody, CallScope $scope): HttpResponse
    {
        $scope->check(); $this->calls++; $this->requests[] = $request;
        return ($this->respond)($this->calls, $request, $scope);
    }
    public function close(): void {}
}
function jobs(Script $transport): JobsClient { return new JobsClient(new ClientConfig(endpoint: 'http://127.0.0.1', transport: $transport, maxAttempts: 1)); }
$input = new Input(names: ['a', 'b']);

// Retry on a modeled error type, then on a non-matching output, then succeed.
$transport = new Script(static fn(int $call) => $call === 1
    ? new HttpResponse(404, [], '{"__type":"Missing"}')
    : new HttpResponse(200, [], $call >= 3 ? '{"wire-status":"READY"}' : '{"wire-status":"PENDING"}'));
$output = jobs($transport)->waitUntilReady($input, 1.0, 0.0, 0.0);
check($output?->status === 'READY' && $transport->calls === 3, 'retry then success');
check(json_decode($transport->requests[0]->body, true) === ['names' => ['a', 'b']], 'waiter sends the input snapshot');

// An error acceptor in the success state returns no output.
$transport = new Script(static fn() => new HttpResponse(404, [], '{"__type":"Missing"}'));
check(jobs($transport)->waitUntilDeleted($input, 1.0, 0.0, 0.0) === null && $transport->calls === 1, 'error success');

// Projections, filters, input and output, timestamps as epoch numbers, and the function library.
$transport = new Script(static fn() => new HttpResponse(200, [], '{"items":[{"wire-status":"READY"},{"wire-status":"READY"}],"updatedAt":"2026-01-01T00:00:00Z"}'));
$client = jobs($transport);
foreach (['waitUntilAllReady', 'waitUntilAnyReady', 'waitUntilFilteredReady', 'waitUntilCountsMatch', 'waitUntilUpdated', 'waitUntilFunctions'] as $method) {
    check($client->$method($input, 1.0, 0.0, 0.0)?->items[0]->status === 'READY', $method);
}
check($transport->calls === 6, 'each query waiter succeeds on its first attempt');
check($input->names === ['a', 'b'], 'waiter input is not mutated');
$transport = new Script(static fn() => new HttpResponse(200, [], '{"items":[{"wire-status":"READY"},{"wire-status":"PENDING"}]}'));
$client = jobs($transport);
check($client->waitUntilAnyReady($input, 1.0, 0.0, 0.0) !== null, 'any matches one');
try { $client->waitUntilAllReady($input, 0.05, 0.0, 0.0); throw new RuntimeException('all should not match'); } catch (WaiterTimeoutException) {}
try { $client->waitUntilCountsMatch(new Input(names: ['a']), 0.05, 0.0, 0.0); throw new RuntimeException('counts should differ'); } catch (WaiterTimeoutException) {}

// A failure acceptor reports its modeled message.
$transport = new Script(static fn() => new HttpResponse(200, [], '{"wire-status":"FAILED","reasons":["job rejected"]}'));
try { jobs($transport)->waitUntilReady($input, 1.0); throw new RuntimeException('expected failure'); }
catch (WaiterTimeoutException $error) { throw $error; }
catch (WaiterException $error) {
    check($error->attempts === 1 && $error->reason === ['job rejected'] && str_contains($error->getMessage(), 'job rejected'), 'failure message');
    check($error->response?->status === 'FAILED', 'failure response');
}

// An unmatched error is terminal and keeps the cause.
$transport = new Script(static fn() => new HttpResponse(403, [], '{"__type":"Denied"}'));
try { jobs($transport)->waitUntilReady($input, 1.0); throw new RuntimeException('expected terminal error'); }
catch (WaiterException $error) {
    check(!$error instanceof WaiterTimeoutException && $error->getPrevious() instanceof ServiceError && $error->getPrevious()->metadata->name === 'Denied', 'terminal error');
}

// The total deadline bounds polling, retry delays, and an in-flight attempt.
$transport = new Script(static fn() => new HttpResponse(200, [], '{"wire-status":"PENDING"}'));
$start = hrtime(true);
try { jobs($transport)->waitUntilReady($input, 0.05); throw new RuntimeException('expected timeout'); }
catch (WaiterTimeoutException $error) { check($error->attempts === 1 && $transport->calls === 1, 'deadline with one-second minimum delay'); }
check((hrtime(true) - $start) / 1e9 < 0.5, 'deadline ends polling');
$transport = new Script(static function(int $call, HttpRequest $request, CallScope $scope): never { while (true) { $scope->check(); usleep(1000); } });
try { jobs($transport)->waitUntilReady($input, 0.05); throw new RuntimeException('expected in-flight timeout'); }
catch (WaiterTimeoutException $error) { check($error->getPrevious() !== null, 'in-flight attempt is bounded by the waiter deadline'); }

// Cancellation propagates rather than becoming a waiter failure.
$cancel = new CancellationToken(); $cancel->cancel();
try { jobs(new Script(static fn() => throw new RuntimeException('dispatched')))->waitUntilReady($input, 1.0, cancellation: $cancel); throw new RuntimeException('expected cancellation'); }
catch (CancelledException) {}
$cancelled = false;
$transport = new Script(static function() use (&$cancelled) { $cancelled = true; return new HttpResponse(200, [], '{"wire-status":"PENDING"}'); });
try { jobs($transport)->waitUntilReady($input, 5.0, 1.0, 1.0, new CancellationToken(static function() use (&$cancelled) { return $cancelled; })); throw new RuntimeException('expected cancellation during delay'); }
catch (CancelledException) { check($transport->calls === 1, 'cancelled during the delay'); }

// Delay validation, exponential backoff with jitter bounds, and the final delay ending at the deadline.
foreach ([[0.0, null, null], [INF, null, null], [1.0, 2.0, 1.0], [1.0, -1.0, null]] as [$timeout, $min, $max]) {
    try { jobs(new Script(static fn() => new HttpResponse(200)))->waitUntilReady($input, $timeout, $min, $max); throw new RuntimeException('accepted invalid delays'); }
    catch (InvalidArgumentException) {}
}
$schema = JobsSdk\Schema::codec()->schema->get('operations')->get('Read')->get('waiters')->get('Ready');
$waiter = new Waiter('Ready', new JsonObject([...$schema->members, 'minDelay' => new JsonNumber('2'), 'maxDelay' => new JsonNumber('8')]), 100.0, random: static fn(float $min, float $max) => $max);
check(array_map(static fn(int $attempt) => $waiter->delay($attempt, 100.0), [1, 2, 3, 4, 5]) === [2.0, 4.0, 8.0, 8.0, 8.0], 'backoff ceiling');
check($waiter->delay(2, 5.0) === 3.0, 'last delay reserves a call budget');
$low = new Waiter('Ready', $schema, 100.0, random: static fn(float $min, float $max) => $min);
check($low->minDelay === 1.0 && $low->maxDelay === 4.0 && $low->delay(3, 100.0) === 1.0, 'modeled delays and jitter floor');
for ($i = 0; $i < 50; $i++) { $delay = (new Waiter('Ready', $schema, 100.0))->delay(3, 100.0); check($delay >= 1.0 && $delay <= 4.0, 'jitter range'); }

// The compiled query keeps exact numbers and distinguishes booleans from numbers.
$literal = static fn(string $json) => new JsonObject(['kind' => 'literal', 'value' => ExactJson::decode($json)]);
$call = static fn(string $name, JsonObject ...$args) => Query::search(new JsonObject(['kind' => 'function', 'function' => $name, 'arguments' => $args]), null);
check($call('to_number', new JsonObject(['kind' => 'literal', 'value' => '12345678901234567890.0000000001']))->text === '12345678901234567890.0000000001', 'exact to_number');
check($call('contains', $literal('[true]'), $literal('1')) === false, 'boolean is not a number');
check(array_map(static fn($n) => $n->text, $call('sort', $literal('[2.2,1.1,10]'))) === ['1.1', '2.2', '10'], 'numeric sort');
check($call('type', $literal('1.1')) === 'number' && $call('type', $literal('{}')) === 'object' && $call('type', $literal('[]')) === 'array', 'types');
check($call('to_string', $literal('{"number":1.000000000000000000001}')) === '{"number":1.000000000000000000001}', 'exact to_string');
check($call('sum', $literal('[0.1,0.2,-0.3]'))->text === '0' && $call('sum', $literal('[1e2,0.5]'))->text === '100.5', 'exact sum');
check($call('avg', $literal('[1,2,2]'))->text === '1.6666666666666666666666666666666667', 'avg precision');
check($call('ceil', $literal('1.5'))->text === '2' && $call('floor', $literal('-0.5'))->text === '-1' && $call('ceil', $literal('3'))->text === '3', 'rounding');
check($call('avg', $literal('[]')) === null && $call('max', $literal('[]')) === null, 'empty aggregates');
check($call('length', new JsonObject(['kind' => 'literal', 'value' => "\u{1F600}\u{E9}"]))->text === '2', 'code point length');
try { $call('max', $literal('[1,"a"]')); throw new RuntimeException('mixed max accepted'); } catch (QueryException) {}
try { $call('abs', $literal('"x"')); throw new RuntimeException('abs string accepted'); } catch (QueryException) {}
$slice = static fn(string $json, ?int $start, ?int $stop, ?int $step) => array_map(static fn($n) => $n->text, Query::search(new JsonObject(['kind' => 'slice',
    'start' => $start === null ? null : new JsonNumber((string)$start), 'stop' => $stop === null ? null : new JsonNumber((string)$stop), 'step' => $step === null ? null : new JsonNumber((string)$step)]), ExactJson::decode($json)));
check($slice('[0,1,2,3,4]', 1, null, null) === ['1', '2', '3', '4'] && $slice('[0,1,2,3,4]', null, null, -2) === ['4', '2', '0'] && $slice('[0,1,2,3,4]', -2, null, 1) === ['3', '4'], 'slices');
check($waiter->delay(2, 1.0) === 1.0, 'short budget is exhausted without another poll');
$transport = new Script(static function(int $call): HttpResponse {
    if ($call === 2) usleep(100000);
    return new HttpResponse(200, [], $call === 1 ? '{"wire-status":"PENDING"}' : '{"wire-status":"READY"}');
});
check(jobs($transport)->waitUntilReady($input, 1.5, 1.0, 1.0)?->status === 'READY' && $transport->calls === 2, 'final poll retains a call budget');

echo "PHP waiters, compiled JMESPath, and waiter timing passed\n";
