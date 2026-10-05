<?php
declare(strict_types=1);
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'ChecksumSdk\\')) require getenv('PHP_CHECKSUM_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 12)) . '.php';
});
use ChecksumSdk\{Algorithm, ChecksumsClient, Input};
use ChecksumSdk\Runtime\{Absent, CallScope, ChecksumException, Checksums, ClientConfig, HttpRequest, HttpResponse, HttpTransport};
error_reporting(E_ALL);
set_error_handler(static function(int $severity, string $message, string $file, int $line): never { throw new ErrorException($message, 0, $severity, $file, $line); });
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException($message); }
$vectors = json_decode(file_get_contents(getenv('CHECKSUM_VECTORS')), true, flags: JSON_THROW_ON_ERROR);
$data = $vectors['inputUtf8'];

// Published vectors for every supported algorithm, including the pure-PHP CRC-64/NVME.
foreach ($vectors['base64'] as $algorithm => $expected) check(Checksums::digest($algorithm, $data) === $expected, 'vector ' . $algorithm);
check(Checksums::digest('MD5', '') === $vectors['emptyMd5'], 'empty MD5');
check(Checksums::digest('CRC64NVME', '') === 'AAAAAAAAAAA=' && Checksums::digest('CRC64NVME', str_repeat("\xff", 1000)) === Checksums::digest('CRC64NVME', str_repeat("\xff", 1000)), 'CRC-64 edge inputs');
try { Checksums::digest('XXHASH64', $data); throw new RuntimeException('unsupported digest accepted'); } catch (UnexpectedValueException) {}

final class Server implements HttpTransport {
    public string $mode = 'normal';
    public ?HttpRequest $observed = null;
    public function __construct(private array $vectors) {}
    public function send(HttpRequest $request, bool $readBody, CallScope $scope): HttpResponse
    {
        $this->observed = $request;
        $algorithm = $request->headers['x-algorithm'] ?? null;
        if (!in_array($this->mode, ['override', 'skip'], true)) {
            $name = $algorithm === null ? 'content-md5' : 'x-amz-checksum-' . strtolower($algorithm);
            $expected = ($request->body ?? '') === '' ? $this->vectors['emptyMd5'] : $this->vectors['base64'][$algorithm ?? 'MD5'];
            check(($request->headers[$name] ?? null) === $expected, 'request checksum ' . $name);
        }
        check($readBody, 'validated responses are read');
        $headers = ['x-amz-checksum-xxhash3' => 'unmodeled'];
        $selected = $algorithm ?? 'CRC32';
        if ($this->mode !== 'unmodeled') $headers['x-amz-checksum-' . strtolower($selected)] = $this->mode === 'bad' ? 'AAAA' : $this->vectors['base64'][$selected];
        if ($this->mode === 'one') $headers['x-amz-checksum-sha256'] = 'AAAA';
        return new HttpResponse(200, $headers, $this->vectors['inputUtf8']);
    }
    public function close(): void {}
}
$server = new Server($vectors);
$validated = [];
$client = new ChecksumsClient(new ClientConfig(endpoint: 'http://127.0.0.1', transport: $server, checksumValidated: static function(string $operation, string $algorithm) use (&$validated): void { $validated[] = "$operation:$algorithm"; }));
$client->legacy(new Input(data: $data));
$client->required(new Input(data: $data));
$client->legacy(new Input());
check($server->observed->body === null && $server->observed->headers['content-md5'] === $vectors['emptyMd5'], 'empty body MD5');
$input = new Input(data: $data);
check($client->flexible($input)->data === $data, 'flexible round trip');
check($input->algorithm === Absent::Value && $input->validation === Absent::Value, 'defaults stay private to the call');
check($server->observed->headers['x-algorithm'] === 'CRC32' && $server->observed->headers['x-validation'] === 'ENABLED', 'default algorithm and validation mode');
check(end($validated) === 'Flexible:CRC32', 'validation callback');
foreach (array_keys($vectors['base64']) as $algorithm) {
    $client->flexible(new Input(data: $data, algorithm: new Algorithm($algorithm)));
    check(end($validated) === 'Flexible:' . $algorithm, 'validated ' . $algorithm);
}
$server->mode = 'bad';
try { $client->flexible($input); throw new RuntimeException('corruption accepted'); } catch (ChecksumException $error) { check($error->algorithm === 'CRC32', 'mismatch algorithm'); }
$server->mode = 'one';
$client->flexible($input);
$server->mode = 'unmodeled';
$count = count($validated); $client->flexible($input); check(count($validated) === $count, 'unmodeled checksum is not validated');
$server->mode = 'override';
$client->flexible(new Input(data: $data, checksum: 'supplied'));
check($server->observed->headers['x-amz-checksum-crc32'] === 'supplied', 'caller checksum wins');
$client->legacy(new Input(data: $data, md5: 'supplied'));
check($server->observed->headers['content-md5'] === 'supplied', 'caller MD5 wins');
try { $client->flexible(new Input(data: $data, algorithm: new Algorithm('XXHASH64'))); throw new RuntimeException('unsupported algorithm accepted'); } catch (UnexpectedValueException $error) { check(!$error instanceof ChecksumException, 'rejected before dispatch'); }
$server->mode = 'skip';
$optOut = new ChecksumsClient(new ClientConfig(endpoint: 'http://127.0.0.1', transport: $server, requestChecksumCalculation: Checksums::WHEN_REQUIRED, responseChecksumValidation: Checksums::WHEN_REQUIRED));
$optOut->flexible(new Input(data: $data));
check(!isset($server->observed->headers['x-algorithm']) && !isset($server->observed->headers['x-validation']) && !isset($server->observed->headers['x-amz-checksum-crc32']), 'opt out');
$optOut->required(new Input(data: $data));
check($server->observed->headers['content-md5'] === $vectors['base64']['MD5'], 'required checksums ignore the opt out');
try { new ClientConfig(requestChecksumCalculation: 'ALWAYS'); throw new RuntimeException('invalid policy accepted'); } catch (InvalidArgumentException) {}
$server->mode = 'normal';
$hooked = new ChecksumsClient(new ClientConfig(endpoint: 'http://127.0.0.1', transport: $server, middleware: [static fn(HttpRequest $request) => $request->withBody($data)]));
$hooked->legacy(new Input());
check($server->observed->body === $data, 'checksum covers the final middleware bytes');
echo "PHP checksum vectors, request checksums, and response validation passed\n";
