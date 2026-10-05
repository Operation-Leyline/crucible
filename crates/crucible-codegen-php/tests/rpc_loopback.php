<?php
declare(strict_types=1);
// RPC v2 CBOR, HTTP Digest, and SigV4a over the real cURL transport against rpc_server.php,
// which checks framing, verifies digest responses, and verifies SigV4a signatures itself.
spl_autoload_register(static function(string $class): void {
    if (str_starts_with($class, 'VaultSdk\\')) require getenv('PHP_VAULT_PACKAGE') . '/src/' . str_replace('\\', '/', substr($class, 9)) . '.php';
});
use VaultSdk\{ChoiceText, Denied, Record, SignInput, VaultClient};
use VaultSdk\Runtime\{AwsCredentials, BasicCredentials, BigDecimal, BigInteger, ClientConfig, Timestamp, Unit, UnknownServiceError};
error_reporting(E_ALL);
set_error_handler(static function(int $severity, string $message, string $file, int $line): never { throw new ErrorException($message, 0, $severity, $file, $line); });
function check(bool $condition, string $message): void { if (!$condition) throw new RuntimeException($message); }

$server = proc_open([PHP_BINARY, __DIR__ . '/rpc_server.php'], [1 => ['pipe', 'w'], 2 => STDERR], $pipes);
if (!is_resource($server)) throw new RuntimeException('Cannot start the RPC server');
try {
    $line = fgets($pipes[1]);
    check($line !== false && str_starts_with($line, 'listening '), 'RPC server started');
    $endpoint = 'http://' . trim(substr($line, 10)) . '/';
    $log = static function(int $count) use ($pipes): array { $lines = []; for ($i = 0; $i < $count; $i++) $lines[] = trim(fgets($pipes[1])); return $lines; };
    $client = static fn(array $settings = []) => new VaultClient(new ClientConfig(...['endpoint' => $endpoint, 'maxAttempts' => 1,
        'digestAuth' => static fn() => new BasicCredentials('Mufasa', 'Circle of Life'), ...$settings]));

    check($client()->ping() instanceof Unit && $log(1) === ['Ping 200'], 'Unit RPC over HTTP');
    try { $client(['headers' => ['x-mode' => 'malformed']])->ping(); throw new RuntimeException('expected a malformed response'); }
    catch (UnexpectedValueException) { check($log(1) === ['Ping 200'], 'a response without Smithy-Protocol is rejected'); }

    // Digest: challenge, answer, preemptive nonce counts, and a stale nonce after three uses.
    $vault = $client();
    $record = new Record(key: 'k', data: random_bytes(300), when: new Timestamp(1700000000, 123000000), count: new BigInteger('123456789012345678901234567890'),
        ratio: new BigDecimal('3.14159265358979323846264338327950288'), small: PHP_INT_MIN, score: -0.5, level: 0.25, tags: ['x' => 'y'], items: ['', "\0"], choice: new ChoiceText('t'));
    $output = $vault->put($record);
    check($log(2) === ['Put 401 none', 'Put 200 SHA-256 auth 00000001'], 'challenge, then the SHA-256 answer');
    check($output->data === $record->data && $output->when == $record->when && $output->count->equals($record->count) && $output->ratio->equals($record->ratio)
        && $output->small === PHP_INT_MIN && $output->score === -0.5 && $output->level === 0.25 && $output->tags === ['x' => 'y'] && $output->items === ['', "\0"]
        && $output->choice instanceof ChoiceText && $output->choice->value === 't', 'CBOR record round trip over HTTP');
    $vault->put(new Record(key: 'b'));
    $vault->put(new Record(key: 'c'));
    check($log(2) === ['Put 200 SHA-256 auth 00000002', 'Put 200 SHA-256 auth 00000003'], 'preemptive answers with increasing nonce counts');
    $vault->put(new Record(key: 'd'));
    check($log(2) === ['Put 401 stale', 'Put 200 SHA-256 auth 00000001'], 'a stale nonce is replaced without new credentials');
    foreach (['MD5' => 'MD5 auth', 'MD5-sess' => 'MD5-sess auth', 'SHA-256-sess' => 'SHA-256-sess auth', 'SHA-512-256' => 'SHA-512-256 auth', 'SHA-256/auth-int' => 'SHA-256 auth-int'] as $mode => $expected) {
        $client(['headers' => ['x-digest' => $mode]])->put(new Record(key: 'm', data: 'body'));
        check($log(2) === ['Put 401 none', "Put 200 $expected 00000001"], "digest $mode");
    }
    try { $client(['digestAuth' => static fn() => new BasicCredentials('Mufasa', 'wrong')])->put(new Record(key: 'k')); throw new RuntimeException('expected 401'); }
    catch (UnknownServiceError $error) { check($error->metadata->status === 401 && $log(2) === ['Put 401 none', 'Put 401 bad'], 'wrong password fails after one answer'); }
    try { $client(['headers' => ['x-mode' => 'deny']])->put(new Record(key: 'k')); throw new RuntimeException('expected Denied'); }
    catch (Denied $denied) { check($denied->getMessage() === 'denied by policy' && $denied->metadata->status === 403 && $log(2)[1] === 'Put 403 SHA-256 auth 00000001', 'modeled CBOR error over HTTP'); }

    // SigV4a: the server rebuilds the canonical request and verifies with its own derived key.
    $credentials = static fn() => new AwsCredentials('AKIDEXAMPLE', 'wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY');
    $signed = $client(['credentials' => $credentials, 'region' => 'us-east-1', 'sigv4aSigningRegionSet' => ['us-east-1', 'us-west-2']])->sign(new SignInput(value: 'payload'));
    check($signed->value === 'verified' && $signed->regionSet === 'us-east-1,us-west-2' && $log(1) === ['Sign verified'], 'SigV4a verified by the server');
    $session = $client(['credentials' => static fn() => new AwsCredentials('AKIDEXAMPLE', 'wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY', 'session-token'), 'region' => 'eu-west-1'])->sign(new SignInput());
    check($session->regionSet === 'eu-west-1' && $log(1) === ['Sign verified'], 'SigV4a with a session token and the client region');
    try { $client(['credentials' => static fn() => new AwsCredentials('AKIDEXAMPLE', 'another-secret'), 'region' => 'us-east-1'])->sign(new SignInput(value: 'v')); throw new RuntimeException('expected Denied'); }
    catch (Denied $denied) { check($denied->getMessage() === 'signature' && $log(1) === ['Sign signature'], 'a signature from another key is rejected'); }
    echo "PHP RPC v2 CBOR, HTTP Digest, and SigV4a over cURL passed\n";
} finally {
    proc_terminate($server);
    proc_close($server);
}
