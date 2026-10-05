<?php
declare(strict_types=1);
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'StreamSdk\\')) require getenv('PHP_STREAM_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 10)) . '.php';
});
use StreamSdk\{Algorithm, Decoder, FetchInput, Schema, SizedInput, StoreInput, StreamingClient, UploadInput};
use StreamSdk\Runtime\{Authentication, AwsCredentials, ByteStream, CallScope, ChecksumException, ChecksumHasher, Checksums, ClientConfig, ClientRuntime, HttpRequest, HttpResponse, HttpTransport, NetworkException, ServiceError, StreamException, Unit};
error_reporting(E_ALL);
set_error_handler(static function(int $severity, string $message, string $file, int $line): never { throw new ErrorException($message, 0, $severity, $file, $line); });
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException($message); }
function rejects(string $class, callable $work): Throwable
{
    try { $work(); } catch (Throwable $error) { if ($error instanceof $class) return $error; throw $error; }
    throw new RuntimeException('Expected ' . $class);
}
$vectors = json_decode(file_get_contents(getenv('CHECKSUM_VECTORS')), true, flags: JSON_THROW_ON_ERROR);

/** Streams request bodies in small reads and serves responses through the sink, like a streaming transport. */
final class Fake implements HttpTransport {
    /** @var list<HttpRequest> */
    public array $requests = [];
    /** @var list<?string> */
    public array $bodies = [];
    public function __construct(private Closure $handler) {}
    public function send(HttpRequest $request, bool $readBody, CallScope $scope, ?Closure $sink = null): HttpResponse
    {
        $this->requests[] = $request;
        if ($request->stream !== null) {
            check($request->body === null, 'a streaming request has no byte body');
            $body = $request->stream->open(); $data = '';
            try { while (($chunk = $body->read(7)) !== '') $data .= $chunk; } finally { $body->close(); }
            $this->bodies[] = $data;
        } else $this->bodies[] = $request->body;
        return ($this->handler)(count($this->requests), $request, $sink);
    }
    public function close(): void {}
}
/** A transport written before streaming: it never receives a sink and buffers the body. */
final class Buffered implements HttpTransport {
    public function __construct(private HttpResponse $response) {}
    public function send(HttpRequest $request, bool $readBody, CallScope $scope): HttpResponse { return $this->response; }
    public function close(): void {}
}
final class Psr7Like {
    private int $position = 0;
    public function __construct(private string $data) {}
    public function isSeekable(): bool { return true; }
    public function tell(): int { return $this->position; }
    public function seek(int $offset): void { $this->position = $offset; }
    public function getSize(): int { return strlen($this->data); }
    public function eof(): bool { return $this->position >= strlen($this->data); }
    public function read(int $length): string { $chunk = substr($this->data, $this->position, $length); $this->position += strlen($chunk); return $chunk; }
}
function sdk(HttpTransport $transport, array $options = []): StreamingClient
{
    return new StreamingClient(new ClientConfig(...['endpoint' => 'https://example.invalid', 'transport' => $transport, 'baseDelay' => 0.0, ...$options]));
}
function generate(array $chunks): Generator { foreach ($chunks as $chunk) yield $chunk; }
$retrying = static fn(int $count) => new HttpResponse($count === 1 ? 503 : 200);
$data = 'streamed bytes';

// Replayable sources reopen per attempt and send a known length.
$fake = new Fake($retrying);
sdk($fake)->upload(new UploadInput(body: ByteStream::fromString($data)));
check($fake->bodies === [$data, $data] && array_map(static fn($r) => $r->headers['content-length'], $fake->requests) === [(string)strlen($data), (string)strlen($data)], 'strings replay with a length');
check($fake->requests[0]->headers['content-type'] === 'application/octet-stream' && !isset($fake->requests[0]->headers['transfer-encoding']), 'sized stream headers');
$handle = fopen('php://memory', 'w+b'); fwrite($handle, 'skip' . $data); rewind($handle); fread($handle, 4);
$fake = new Fake($retrying);
sdk($fake)->upload(new UploadInput(body: ByteStream::fromResource($handle)));
check($fake->bodies === [$data, $data] && $fake->requests[0]->headers['content-length'] === (string)strlen($data) && is_resource($handle), 'seekable resources replay from their position and stay open');
$opened = 0;
$fake = new Fake($retrying);
sdk($fake)->upload(new UploadInput(body: ByteStream::fromFactory(static function() use (&$opened, $data) { $opened++; $h = fopen('php://memory', 'w+b'); fwrite($h, $data); rewind($h); return $h; }, strlen($data))));
check($fake->bodies === [$data, $data] && $opened === 2, 'factories open once per attempt');
$file = tempnam(sys_get_temp_dir(), 'crucible-stream-'); file_put_contents($file, 'file bytes');
try {
    $fake = new Fake($retrying);
    sdk($fake)->upload(new UploadInput(body: ByteStream::fromFile($file)));
    check($fake->bodies === ['file bytes', 'file bytes'] && $fake->requests[1]->headers['content-length'] === '10', 'files reopen per attempt');
} finally { unlink($file); }
$fake = new Fake($retrying);
sdk($fake)->upload(new UploadInput(body: ByteStream::fromObject(new Psr7Like($data))));
check($fake->bodies === [$data, $data] && $fake->requests[0]->headers['content-length'] === (string)strlen($data), 'PSR-7 style streams replay');

// One-shot sources get exactly one attempt and are chunked when their length is unknown.
$oneShot = ByteStream::fromIterable(generate(['one ', 'shot']));
check(!$oneShot->replayable && $oneShot->length === null, 'one-shot properties');
$fake = new Fake($retrying);
check(rejects(ServiceError::class, static fn() => sdk($fake)->upload(new UploadInput(body: $oneShot)))->metadata->status === 503, 'one-shot failure surfaces');
check(count($fake->requests) === 1 && $fake->bodies === ['one shot'] && $fake->requests[0]->headers['transfer-encoding'] === 'chunked' && !isset($fake->requests[0]->headers['content-length']), 'one-shot is sent once, chunked');
rejects(LogicException::class, static fn() => sdk(new Fake($retrying))->upload(new UploadInput(body: $oneShot)));
$fake = new Fake(static fn() => new HttpResponse(200));
sdk($fake, ['middleware' => [static function(HttpRequest $request): HttpRequest { check($request->stream !== null && $request->body === null, 'middleware sees the stream'); return $request; }]])->upload(new UploadInput(body: ByteStream::fromString('x')));
rejects(InvalidArgumentException::class, static fn() => ByteStream::fromString('x')->withSha256('nope'));

// @requiresLength and @length are checked before dispatch and while bytes flow.
$fake = new Fake(static fn() => new HttpResponse(200));
rejects(InvalidArgumentException::class, static fn() => sdk($fake)->sized(new SizedInput(body: ByteStream::fromIterable(generate(['abc'])))));
rejects(InvalidArgumentException::class, static fn() => sdk($fake)->sized(new SizedInput(body: ByteStream::fromString('abcd'))));
check($fake->requests === [], 'length rules are checked before dispatch');
sdk($fake)->sized(new SizedInput(body: ByteStream::fromIterable(generate(['a', 'bc']), 3)));
check($fake->bodies === ['abc'] && $fake->requests[0]->headers['content-length'] === '3', 'declared length satisfies requiresLength');
rejects(StreamException::class, static fn() => sdk(new Fake(static fn() => new HttpResponse(200)))->sized(new SizedInput(body: ByteStream::fromIterable(generate(['abcd']), 3))));
rejects(StreamException::class, static fn() => sdk(new Fake(static fn() => new HttpResponse(200)))->sized(new SizedInput(body: ByteStream::fromIterable(generate(['ab']), 3))));
$failing = new Fake($retrying);
rejects(StreamException::class, static fn() => sdk($failing)->upload(new UploadInput(body: ByteStream::fromFactory(static function() { yield 'partial'; throw new RuntimeException('source broke'); }))));
check(count($failing->requests) === 1, 'a failed source is not retried');

// Request checksums read a replayable stream in chunks; a one-shot stream needs the caller's header.
$payload = $vectors['inputUtf8'];
$fake = new Fake(static fn() => new HttpResponse(200));
sdk($fake)->store(new StoreInput(body: ByteStream::fromString($payload)));
check(end($fake->requests)->headers['x-amz-checksum-crc32'] === $vectors['base64']['CRC32'] && end($fake->bodies) === $payload, 'checksum precedes a replayable stream');
rejects(InvalidArgumentException::class, static fn() => sdk($fake)->store(new StoreInput(body: ByteStream::fromIterable(generate([$payload])))));
sdk($fake)->store(new StoreInput(body: ByteStream::fromIterable(generate([$payload])), checksum: 'supplied'));
check(end($fake->requests)->headers['x-amz-checksum-crc32'] === 'supplied' && end($fake->bodies) === $payload, 'caller checksum lets a one-shot stream through');
sdk($fake, ['requestChecksumCalculation' => Checksums::WHEN_REQUIRED])->store(new StoreInput(body: ByteStream::fromIterable(generate([$payload]))));
check(!isset(end($fake->requests)->headers['x-amz-checksum-crc32']), 'opted-out checksum');
foreach ($vectors['base64'] as $algorithm => $expected) {
    $hasher = new ChecksumHasher($algorithm);
    foreach (str_split($payload) as $character) $hasher->update($character);
    check($hasher->base64() === $expected, 'incremental ' . $algorithm);
    check(Checksums::stream($algorithm, ByteStream::fromIterable([substr($payload, 0, 4), substr($payload, 4)])) === $expected, 'streamed ' . $algorithm);
}

// Digests and SigV4 never buffer a one-shot stream.
$identity = new AwsCredentials('key', 'secret');
$signed = Authentication::sign(new HttpRequest('https://example.invalid/', 'PUT', [], null, ByteStream::fromString($payload)), $identity, 'example', 'local');
check($signed->headers['x-amz-content-sha256'] === 'UNSIGNED-PAYLOAD', 'streams sign as UNSIGNED-PAYLOAD');
$declared = hash('sha256', $payload);
$signed = Authentication::sign(new HttpRequest('https://example.invalid/', 'PUT', [], null, ByteStream::fromString($payload)->withSha256($declared)), $identity, 'example', 'local');
check($signed->headers['x-amz-content-sha256'] === $declared, 'a declared SHA-256 is signed');
$digesting = static fn(HttpTransport $transport) => new ClientRuntime(new ClientConfig(endpoint: 'https://example.invalid', transport: $transport), Schema::codec(), Decoder::error(...), '', true);
$fake = new Fake(static fn() => new HttpResponse(200));
$digesting($fake)->call('Upload', new UploadInput(body: ByteStream::fromString($payload)));
check(end($fake->requests)->headers['x-amz-content-sha256'] === $declared, 'replayable streams are hashed in chunks');
rejects(InvalidArgumentException::class, static fn() => $digesting($fake)->call('Upload', new UploadInput(body: ByteStream::fromIterable(generate([$payload])))));
$digesting($fake)->call('Upload', new UploadInput(body: ByteStream::fromIterable(generate([$payload]))->withSha256($declared)));
check(end($fake->requests)->headers['x-amz-content-sha256'] === $declared && end($fake->bodies) === $payload, 'declared digest for a one-shot stream');

// Streamed responses go to the target as they arrive, with an incremental checksum.
$chunked = new Fake(static function(int $count, HttpRequest $request, ?Closure $sink) {
    if ($count === 1) return new HttpResponse(503);
    $headers = ['x-count' => '3', 'x-amz-checksum-crc32' => Checksums::digest('CRC32', 'abcde')];
    foreach (['ab', 'cd', 'e'] as $chunk) $sink($chunk, $headers);
    return new HttpResponse(200, $headers);
});
$received = [];
$output = sdk($chunked)->download(responseTarget: static function(string $chunk) use (&$received): void { $received[] = $chunk; });
check($received === ['ab', 'cd', 'e'] && $output->count === 3 && $output->body->read() === '', 'closure receives chunks after a retry');
$target = fopen('php://memory', 'w+b');
$serve = static fn(string $body) => new Fake(static function(int $count, HttpRequest $request, ?Closure $sink) use ($body) { $sink($body, []); return new HttpResponse(200); });
check(sdk($serve('xyz'))->download(responseTarget: $target)->body->resource() === $target && (rewind($target) && stream_get_contents($target) === 'xyz'), 'resource target');
check(sdk($serve('xyz'))->download()->body->getContents() === 'xyz', 'default temporary buffer');
check(sdk(new Buffered(new HttpResponse(200, [], 'buffered')))->download()->body->getContents() === 'buffered', 'transports without a sink still work');
$validated = [];
sdk($chunked, ['checksumValidated' => static function(string $operation, string $algorithm) use (&$validated): void { $validated[] = "$operation:$algorithm"; }])->fetch(new FetchInput(), responseTarget: static fn() => null);
check($validated === ['Fetch:CRC32'] && end($chunked->requests)->headers['x-validation'] === 'ENABLED', 'incremental response checksum');
$corrupt = new Fake(static function(int $count, HttpRequest $request, ?Closure $sink) { $sink('abcde', ['x-amz-checksum-crc32' => 'AAAA']); return new HttpResponse(200, ['x-amz-checksum-crc32' => 'AAAA']); });
rejects(ChecksumException::class, static fn() => sdk($corrupt)->fetch(new FetchInput(), responseTarget: static fn() => null));
$broken = new Fake(static function(int $count, HttpRequest $request, ?Closure $sink) {
    $sink('partial', []);
    if ($count === 1) throw new NetworkException('reset');
    return new HttpResponse(200);
});
rejects(NetworkException::class, static fn() => sdk($broken)->download(responseTarget: static fn() => null));
check(count($broken->requests) === 1, 'delivered response bytes are never retried');
$broken = new Fake(static function(int $count, HttpRequest $request, ?Closure $sink) { $sink($count === 1 ? 'partial' : 'whole', []); if ($count === 1) throw new NetworkException('reset'); return new HttpResponse(200); });
check(sdk($broken)->download()->body->getContents() === 'whole' && count($broken->requests) === 2, 'the private buffer is discarded for a retry');
rejects(InvalidArgumentException::class, static fn() => sdk($serve('x'))->download(responseTarget: 42));
rejects(TypeError::class, static fn() => new UploadInput(body: 'not a stream'));
echo "PHP streaming blobs passed\n";
