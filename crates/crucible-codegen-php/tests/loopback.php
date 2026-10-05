<?php
declare(strict_types=1);
// Real cURL transport against a loopback server: streamed bodies with bounded memory,
// cancellation and deadlines between chunks, and waiters and checksums over HTTP.
spl_autoload_register(static function(string $class): void {
    foreach (['StreamSdk' => 'PHP_STREAM_PACKAGE', 'JobsSdk' => 'PHP_JOBS_PACKAGE', 'ChecksumSdk' => 'PHP_CHECKSUM_PACKAGE'] as $namespace => $variable) {
        if (str_starts_with($class, $namespace . '\\')) require getenv($variable) . '/src/' . str_replace('\\', '/', substr($class, strlen($namespace) + 1)) . '.php';
    }
});
use StreamSdk\{FetchInput, StoreInput, StreamingClient, UploadInput};
use StreamSdk\Runtime\{ByteStream, CancellationToken, CancelledException, ChecksumException, ClientConfig, TimeoutException};
error_reporting(E_ALL);
set_error_handler(static function(int $severity, string $message, string $file, int $line): never { throw new ErrorException($message, 0, $severity, $file, $line); });
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException($message); }
function rejects(string $class, callable $work): Throwable
{
    try { $work(); } catch (Throwable $error) { if ($error instanceof $class) return $error; throw $error; }
    throw new RuntimeException('Expected ' . $class);
}
$size = (int)(getenv('STREAM_BYTES') ?: 256 * 1024 * 1024);
$server = proc_open([PHP_BINARY, __DIR__ . '/loopback_server.php', (string)$size], [1 => ['pipe', 'w'], 2 => STDERR], $pipes);
if (!is_resource($server)) throw new RuntimeException('Cannot start the loopback server');
try {
    $line = fgets($pipes[1]);
    check($line !== false && str_starts_with($line, 'listening '), 'loopback server started');
    $endpoint = 'http://' . trim(substr($line, 10));
    $uploaded = static fn() => explode(' ', trim(fgets($pipes[1])));
    $client = new StreamingClient(new ClientConfig(endpoint: $endpoint, timeout: 300));
    $client->upload(new UploadInput(body: ByteStream::fromString('warm')));
    check($uploaded() === ['PUT', '/upload', '4', 'length', base64_encode(hash('crc32b', 'warm', true))], 'small sized upload');

    // 256 MiB each way, never held in memory: chunks are generated and consumed on demand.
    ini_set('memory_limit', '64M');
    $block = str_repeat('x', 65536);
    $chunks = static function() use ($size, $block): Generator {
        for ($left = $size; $left > 0; $left -= strlen($block)) yield $left >= strlen($block) ? $block : substr($block, 0, $left);
    };
    memory_reset_peak_usage();
    $client->upload(new UploadInput(body: ByteStream::fromIterable($chunks())));
    [, , $received, $framing] = $uploaded();
    check((int)$received === $size && $framing === 'chunked', "chunked upload of unknown length ($received bytes)");
    $client->upload(new UploadInput(body: ByteStream::fromFactory($chunks, $size)));
    [, , $received, $framing] = $uploaded();
    check((int)$received === $size && $framing === 'length', "sized upload ($received bytes)");
    $total = 0; $largest = 0;
    $output = $client->download(responseTarget: static function(string $chunk) use (&$total, &$largest): void { $total += strlen($chunk); $largest = max($largest, strlen($chunk)); });
    check($total === $size && $output->count === 1 && $largest <= 1024 * 1024, "streamed download ($total bytes, largest chunk $largest)");
    $peak = memory_get_peak_usage();
    check($peak < 32 * 1024 * 1024, "bounded memory: peak $peak bytes for $size bytes each way");
    ini_set('memory_limit', '256M');

    // A file target and the default buffer receive the body without the caller holding it.
    $target = tmpfile();
    $client->fetch(new FetchInput(), responseTarget: $target);
    rewind($target);
    check(stream_get_contents($target) === '123456789', 'resource target over HTTP');
    $validated = [];
    $checked = new StreamingClient(new ClientConfig(endpoint: $endpoint, checksumValidated: static function(string $operation, string $algorithm) use (&$validated): void { $validated[] = "$operation:$algorithm"; }));
    check($checked->fetch(new FetchInput())->body->getContents() === '123456789' && $validated === ['Fetch:CRC32'], 'incremental response checksum over HTTP');
    rejects(ChecksumException::class, static fn() => (new StreamingClient(new ClientConfig(endpoint: $endpoint, headers: ['x-mode' => 'bad'])))->fetch(new FetchInput()));
    $client->store(new StoreInput(body: ByteStream::fromString('checked body')));
    check($uploaded()[2] === '12', 'replayable stream checksum accepted by the server');

    // Cancellation and the deadline stop a stalled streamed response between chunks.
    $slow = new StreamingClient(new ClientConfig(endpoint: $endpoint, headers: ['x-mode' => 'slow'], timeout: 30));
    $token = new CancellationToken(); $count = 0; $start = hrtime(true);
    try { $slow->fetch(new FetchInput(), $token, static function() use ($token, &$count): void { if (++$count === 2) $token->cancel(); }); throw new RuntimeException('expected cancellation'); }
    catch (CancelledException) {}
    check($count === 2 && (hrtime(true) - $start) / 1e9 < 2, 'cancellation stops a streamed response: ' . $count . ' chunks in ' . (hrtime(true) - $start) / 1e9 . 's');
    $deadline = new StreamingClient(new ClientConfig(endpoint: $endpoint, headers: ['x-mode' => 'slow'], timeout: 0.3));
    $start = hrtime(true);
    rejects(TimeoutException::class, static fn() => $deadline->fetch(new FetchInput(), responseTarget: static fn() => null));
    check((hrtime(true) - $start) / 1e9 < 2, 'the deadline stops a stalled streamed response');

    // Waiters and checksums over the real transport.
    $jobs = new JobsSdk\JobsClient(new JobsSdk\Runtime\ClientConfig(endpoint: $endpoint));
    check($jobs->waitUntilReady(new JobsSdk\Input(names: ['a']), 10.0, 0.0, 0.0)?->status === 'READY', 'waiter polls over HTTP');
    $validated = [];
    $checksums = new ChecksumSdk\ChecksumsClient(new ChecksumSdk\Runtime\ClientConfig(endpoint: $endpoint, checksumValidated: static function(string $operation, string $algorithm) use (&$validated): void { $validated[] = "$operation:$algorithm"; }));
    foreach (['CRC32', 'CRC32C', 'SHA256'] as $algorithm) {
        check($checksums->flexible(new ChecksumSdk\Input(data: '123456789', algorithm: new ChecksumSdk\Algorithm($algorithm)))->data === '123456789', 'checksum round trip ' . $algorithm);
    }
    check($validated === ['Flexible:CRC32', 'Flexible:CRC32C', 'Flexible:SHA256'], 'request and response checksums over HTTP');
    $client->close(); $jobs->close(); $checksums->close();
    echo "PHP loopback HTTP passed: streamed $size bytes each way with peak PHP memory $peak bytes; waiters and checksums over cURL\n";
} finally {
    proc_terminate($server);
    proc_close($server);
}
