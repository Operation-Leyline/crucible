<?php
declare(strict_types=1);
/*
 * A loopback HTTP/1.1 server for the PHP client's real-transport tests. It serves one
 * connection at a time and closes each after its response, reads request bodies in
 * fixed-size pieces, and reports each upload on stdout, so neither side buffers a body.
 */
$size = (int)($argv[1] ?? 0);
$server = stream_socket_server('tcp://127.0.0.1:0', $code, $message);
if ($server === false) { fwrite(STDERR, "listen failed: $message\n"); exit(1); }
fwrite(STDOUT, 'listening ' . stream_socket_get_name($server, false) . "\n");
fflush(STDOUT);
$reads = 0;
function readExactly($client, int $count, ?HashContext $hash): int
{
    $received = 0;
    while ($received < $count) {
        $chunk = fread($client, min(65536, $count - $received));
        if ($chunk === false || ($chunk === '' && feof($client))) break;
        if ($hash !== null) hash_update($hash, $chunk);
        $received += strlen($chunk);
    }
    return $received;
}
function respond($client, int $status, array $headers, string $body = ''): void
{
    $text = "HTTP/1.1 $status " . ($status === 200 ? 'OK' : 'Status') . "\r\nConnection: close\r\nContent-Length: " . strlen($body) . "\r\n";
    foreach ($headers as $name => $value) $text .= "$name: $value\r\n";
    @fwrite($client, $text . "\r\n" . $body);
}
while (true) {
    $client = @stream_socket_accept($server, -1);
    if ($client === false) continue;
    stream_set_timeout($client, 30);
    $line = fgets($client);
    if ($line === false) { fclose($client); continue; }
    [$method, $path] = explode(' ', trim($line)) + ['', ''];
    $headers = [];
    while (($header = fgets($client)) !== false && $header !== "\r\n") {
        [$name, $value] = explode(':', $header, 2) + ['', ''];
        $headers[strtolower(trim($name))] = trim($value);
    }
    $algorithm = $headers['x-algorithm'] ?? 'CRC32';
    $hash = hash_init(['CRC32' => 'crc32b', 'CRC32C' => 'crc32c', 'SHA256' => 'sha256'][$algorithm] ?? 'crc32b');
    $prefix = '';
    $received = 0;
    $chunked = ($headers['transfer-encoding'] ?? '') === 'chunked';
    if ($chunked) {
        while (($sizeLine = fgets($client)) !== false && ($length = hexdec(trim(explode(';', $sizeLine)[0]))) > 0) {
            $received += readExactly($client, (int)$length, $hash);
            fread($client, 2);
        }
        fgets($client);
    } elseif (isset($headers['content-length'])) {
        $received = readExactly($client, (int)$headers['content-length'], $hash);
    }
    $digest = base64_encode(hash_final($hash, true));
    if ($method === 'PUT') { fwrite(STDOUT, "$method $path $received " . ($chunked ? 'chunked' : 'length') . " $digest\n"); fflush(STDOUT); }
    switch ($path) {
        case '/download':
            @fwrite($client, "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: $size\r\nx-count: 1\r\n\r\n");
            $block = str_repeat('y', 65536);
            for ($left = $size; $left > 0; $left -= strlen($block)) if (@fwrite($client, $left >= strlen($block) ? $block : substr($block, 0, $left)) === false) break;
            break;
        case '/fetch':
            if (($headers['x-mode'] ?? '') === 'slow') {
                @fwrite($client, "HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n");
                for ($i = 0; $i < 400; $i++) { if (@fwrite($client, "4\r\nslow\r\n") === false || @fflush($client) === false) break; usleep(50000); }
                break;
            }
            respond($client, 200, ['x-count' => '2', 'x-amz-checksum-crc32' => ($headers['x-mode'] ?? '') === 'bad' ? 'AAAAAA==' : 'y/Q5Jg=='], '123456789');
            break;
        case '/store':
            respond($client, ($headers['x-amz-checksum-crc32'] ?? '') === $digest ? 200 : 400, []);
            break;
        case '/read':
            // Jobs waiter: pending twice, then ready.
            $reads++;
            respond($client, 200, ['content-type' => 'application/json'], json_encode(['wire-status' => $reads % 3 === 0 ? 'READY' : 'PENDING']));
            break;
        case '/flexible':
            // Checksum service: verify the request checksum, then echo a fixed body with its checksum.
            $header = 'x-amz-checksum-' . strtolower($algorithm);
            $body = '123456789';
            respond($client, ($headers[$header] ?? '') === $digest ? 200 : 400, [$header => base64_encode(hash(['CRC32' => 'crc32b', 'CRC32C' => 'crc32c', 'SHA256' => 'sha256'][$algorithm] ?? 'crc32b', $body, true))], $body);
            break;
        default:
            respond($client, 200, []);
    }
    @fclose($client);
}
