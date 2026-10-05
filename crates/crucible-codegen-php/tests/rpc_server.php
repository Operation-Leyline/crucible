<?php
declare(strict_types=1);
/*
 * A loopback RPC v2 CBOR server for the PHP client's real-transport tests. It checks the
 * protocol headers and Content-Length, answers HTTP Digest challenges (RFC 7616) and verifies
 * the response hash itself, and verifies SigV4a signatures by rebuilding the canonical request
 * from the received bytes. It reports each request on stdout as one line.
 */
const USER = 'Mufasa';
const PASSWORD = 'Circle of Life';
const REALM = 'vault@loopback';
const ACCESS_KEY = 'AKIDEXAMPLE';
const SECRET_KEY = 'wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY';
$server = stream_socket_server('tcp://127.0.0.1:0', $code, $message);
if ($server === false) { fwrite(STDERR, "listen failed: $message\n"); exit(1); }
fwrite(STDOUT, 'listening ' . stream_socket_get_name($server, false) . "\n");
fflush(STDOUT);

function report(string $line): void { fwrite(STDOUT, $line . "\n"); fflush(STDOUT); }
function text(string $value): string
{
    $length = strlen($value);
    return ($length < 24 ? chr(0x60 | $length) : "\x78" . chr($length)) . $value;
}
function respond($client, int $status, array $headers, string $body = ''): void
{
    $out = "HTTP/1.1 $status Status\r\nConnection: close\r\nContent-Length: " . strlen($body) . "\r\n";
    foreach ($headers as $name => $value) $out .= "$name: $value\r\n";
    @fwrite($client, $out . "\r\n" . $body);
}
/** @return array<string, string> */
function params(string $header): array
{
    preg_match_all('/([a-z0-9*-]+)=(?:"((?:[^"\\\\]|\\\\.)*)"|([^,\s]*))/i', $header, $matches, PREG_SET_ORDER);
    $result = [];
    foreach ($matches as $match) $result[strtolower($match[1])] = isset($match[3]) && $match[3] !== '' ? $match[3] : stripcslashes($match[2]);
    return $result;
}
/** The SigV4a public key, derived here independently of the client runtime. */
function sigv4aKey(): OpenSSLAsymmetricKey
{
    $n2 = hex2bin('ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc63254f');
    for ($counter = 1; ; $counter++) {
        $k0 = hash_hmac('sha256', pack('N', 1) . 'AWS4-ECDSA-P256-SHA256' . "\0" . ACCESS_KEY . chr($counter) . pack('N', 256), 'AWS4A' . SECRET_KEY, true);
        if (strcmp($k0, $n2) <= 0) break;
    }
    $d = addOne($k0);
    $private = openssl_pkey_new(['ec' => ['curve_name' => 'prime256v1', 'd' => $d]]);
    return openssl_pkey_get_public(openssl_pkey_get_details($private)['key']);
}
function addOne(string $bytes): string
{
    $i = strlen($bytes) - 1;
    while ($bytes[$i] === "\xff") { $bytes[$i] = "\0"; $i--; }
    $bytes[$i] = chr(ord($bytes[$i]) + 1);
    return $bytes;
}
$key = sigv4aKey();
$nonces = []; // nonce => [last nonce count, uses]

while (true) {
    $client = @stream_socket_accept($server, -1);
    if ($client === false) continue;
    stream_set_timeout($client, 30);
    $line = fgets($client);
    if ($line === false) { fclose($client); continue; }
    [$method, $target] = explode(' ', trim($line)) + ['', ''];
    $headers = [];
    while (($header = fgets($client)) !== false && $header !== "\r\n") {
        [$name, $value] = explode(':', $header, 2) + ['', ''];
        $headers[strtolower(trim($name))] = trim($value);
    }
    $length = $headers['content-length'] ?? null;
    $body = $length === null ? '' : (string)stream_get_contents($client, (int)$length);
    $cbor = ['Smithy-Protocol' => 'rpc-v2-cbor', 'Content-Type' => 'application/cbor'];
    $operation = basename($target);
    $protocol = ($headers['smithy-protocol'] ?? '') === 'rpc-v2-cbor' && ($headers['accept'] ?? '') === 'application/cbor' && $method === 'POST'
        && str_starts_with($target, '/service/Vault/operation/') && !isset($headers['x-amz-target']);
    if (!$protocol) { report("$operation 400 protocol"); respond($client, 400, []); fclose($client); continue; }
    if ($operation === 'Ping') {
        $ok = $body === '' && !isset($headers['content-type']);
        report("Ping " . ($ok ? 200 : 400));
        respond($client, $ok ? 200 : 400, ($headers['x-mode'] ?? '') === 'malformed' ? [] : ['Smithy-Protocol' => 'rpc-v2-cbor']);
    } elseif ($operation === 'Put') {
        $framed = ($headers['content-type'] ?? '') === 'application/cbor' && $length !== null && (int)$length === strlen($body) && $body !== '';
        $mode = $headers['x-digest'] ?? 'SHA-256';
        [$algorithm, $qop] = str_contains($mode, '/') ? explode('/', $mode, 2) : [$mode, 'auth'];
        $authorization = $headers['authorization'] ?? '';
        $fields = str_starts_with($authorization, 'Digest ') ? params(substr($authorization, 7)) : [];
        $nonce = $fields['nonce'] ?? '';
        $verdict = 'none';
        if ($fields !== []) {
            $hash = static fn(string $text): string => hash(['MD5' => 'md5', 'SHA-256' => 'sha256', 'SHA-512-256' => 'sha512/256'][preg_replace('/-sess$/i', '', $fields['algorithm'] ?? 'MD5')] ?? 'md5', $text);
            $a1 = $hash(USER . ':' . REALM . ':' . PASSWORD);
            if (str_ends_with(strtolower($fields['algorithm'] ?? ''), '-sess')) $a1 = $hash("$a1:$nonce:" . ($fields['cnonce'] ?? ''));
            $a2 = $hash(($fields['qop'] ?? '') === 'auth-int' ? "$method:$target:" . $hash($body) : "$method:$target");
            $expected = $hash("$a1:$nonce:" . ($fields['nc'] ?? '') . ':' . ($fields['cnonce'] ?? '') . ':' . ($fields['qop'] ?? '') . ":$a2");
            $count = hexdec($fields['nc'] ?? '0');
            if (!isset($nonces[$nonce])) $verdict = 'unknown-nonce';
            elseif (($fields['username'] ?? '') !== USER || ($fields['realm'] ?? '') !== REALM || ($fields['uri'] ?? '') !== $target
                || ($fields['algorithm'] ?? '') !== $algorithm || ($fields['qop'] ?? '') !== $qop || ($fields['opaque'] ?? '') !== 'opaque-value') $verdict = 'fields';
            elseif (!hash_equals($expected, $fields['response'] ?? '')) $verdict = 'bad';
            elseif ($count <= $nonces[$nonce][0]) $verdict = 'replay';
            elseif ($nonces[$nonce][1] >= 3) $verdict = 'stale';
            else { $verdict = 'ok'; $nonces[$nonce] = [$count, $nonces[$nonce][1] + 1]; }
        }
        if ($verdict !== 'ok') {
            $fresh = bin2hex(random_bytes(12));
            $nonces[$fresh] = [0, 0];
            $challenge = 'Digest realm="' . REALM . "\", qop=\"$qop\", algorithm=$algorithm, nonce=\"$fresh\", opaque=\"opaque-value\"" . ($verdict === 'stale' ? ', stale=true' : '');
            // Like RFC 7616 3.9.1, a SHA-256 challenge comes with an MD5 alternative the client should not choose.
            if ($algorithm === 'SHA-256') $challenge .= ', Digest realm="' . REALM . "\", qop=\"$qop\", algorithm=MD5, nonce=\"$fresh\", opaque=\"opaque-value\"";
            report("Put 401 $verdict");
            respond($client, 401, [...$cbor, 'WWW-Authenticate' => $challenge], "\xa0");
        } elseif (!$framed) {
            report('Put 400 framing');
            respond($client, 400, []);
        } elseif (($headers['x-mode'] ?? '') === 'deny') {
            report("Put 403 {$fields['algorithm']} {$fields['qop']} {$fields['nc']}");
            respond($client, 403, $cbor, "\xa2" . text('__type') . text('example.vault#Denied') . text('message') . text('denied by policy'));
        } else {
            report("Put 200 {$fields['algorithm']} {$fields['qop']} {$fields['nc']}");
            respond($client, 200, $cbor, $body);
        }
    } elseif ($operation === 'Sign') {
        $verdict = 'bad-header';
        if (preg_match('/^AWS4-ECDSA-P256-SHA256 Credential=([^\/]+)\/(\d{8})\/vault\/aws4_request, SignedHeaders=([a-z0-9;-]+), Signature=([0-9a-f]+)$/D', $headers['authorization'] ?? '', $m)) {
            [, $access, $date, $signed, $signature] = $m;
            $lines = '';
            foreach (explode(';', $signed) as $name) $lines .= $name . ':' . preg_replace('/\s+/', ' ', $headers[$name] ?? '') . "\n";
            $payload = hash('sha256', $body);
            $canonical = "$method\n$target\n\n$lines\n$signed\n$payload";
            $text = "AWS4-ECDSA-P256-SHA256\n" . ($headers['x-amz-date'] ?? '') . "\n$date/vault/aws4_request\n" . hash('sha256', $canonical);
            $required = ['host', 'x-amz-date', 'x-amz-region-set', 'x-amz-content-sha256', 'content-type', 'smithy-protocol'];
            if ($access !== ACCESS_KEY) $verdict = 'access-key';
            elseif (array_diff($required, explode(';', $signed)) !== []) $verdict = 'unsigned-headers';
            elseif (($headers['x-amz-content-sha256'] ?? '') !== $payload) $verdict = 'payload';
            elseif (openssl_verify($text, hex2bin($signature), $key, OPENSSL_ALGO_SHA256) !== 1) $verdict = 'signature';
            else $verdict = 'verified';
        }
        report("Sign $verdict");
        if ($verdict === 'verified') {
            respond($client, 200, $cbor, "\xa2" . text('regionSet') . text($headers['x-amz-region-set']) . text('value') . text('verified'));
        } else {
            respond($client, 403, $cbor, "\xa2" . text('__type') . text('example.vault#Denied') . text('message') . text($verdict));
        }
    } else {
        respond($client, 404, []);
    }
    fclose($client);
}
