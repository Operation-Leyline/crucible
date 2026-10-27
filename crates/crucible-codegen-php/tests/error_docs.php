<?php
declare(strict_types=1);
require getenv('PHP_PACKAGE') . '/vendor/autoload.php';
use Example\ErrorDocs\TroubleshootingClient;
use Example\ErrorDocs\Runtime\{ClientConfig, ServiceError, UnknownServiceError};

error_reporting(E_ALL);
set_error_handler(static function(int $severity, string $message, string $file, int $line): never {
    throw new ErrorException($message, 0, $severity, $file, $line);
});
$cases = json_decode(file_get_contents($argv[1]), true, flags:JSON_THROW_ON_ERROR)['cases'];
if (($argv[2] ?? '') === 'serve') {
    $server = stream_socket_server('tcp://127.0.0.1:0', $errno, $error);
    if ($server === false) throw new RuntimeException($error);
    echo stream_socket_get_name($server, false), "\n";
    flush();
    try {
        foreach ($cases as $case) {
            $socket = stream_socket_accept($server, 15);
            if ($socket === false) throw new RuntimeException('HTTP fixture accept timed out');
            try {
                $request = '';
                while (($line = fgets($socket)) !== false && $line !== "\r\n") {
                    $request .= $line;
                    if (strlen($request) > 65536) throw new RuntimeException('Request header bound');
                }
                if (!str_starts_with($request, 'GET /inspect HTTP/1.1')) throw new RuntimeException('Expected native GET');
                $headers = "HTTP/1.1 {$case['status']} Fixture\r\nConnection: close\r\nContent-Length: " . strlen($case['body']) . "\r\nx-request-id: documentation-consumer\r\n";
                if ($case['identity'] !== null) $headers .= "x-crucible-error: {$case['identity']}\r\n";
                if ($case['header'] !== null) $headers .= "X-Error-Reason: {$case['header']}\r\n";
                $bytes = $headers . "\r\n" . $case['body'];
                while ($bytes !== '') {
                    $written = fwrite($socket, $bytes);
                    if ($written === false || $written === 0) throw new RuntimeException('Fixture write failed');
                    $bytes = substr($bytes, $written);
                }
            } finally { fclose($socket); }
        }
    } finally { fclose($server); }
    exit(0);
}

$process = proc_open([PHP_BINARY, __FILE__, $argv[1], 'serve'],
    [['pipe','r'], ['pipe','w'], ['pipe','w']], $pipes);
if (!is_resource($process)) throw new RuntimeException('Cannot start native HTTP fixture');
fclose($pipes[0]);
try {
    $line = fgets($pipes[1]);
    $address = $line === false ? '' : trim($line);
    if ($address === '') {
        proc_terminate($process);
        $stderr = trim(stream_get_contents($pipes[2]));
        throw new RuntimeException('No fixture address' . ($stderr === '' ? '' : ': ' . $stderr));
    }
    $client = new TroubleshootingClient(new ClientConfig(endpoint:'http://' . $address, maxAttempts:1, timeout:5));
    foreach ($cases as $case) {
        try { $client->inspect(); throw new RuntimeException('Expected service failure'); }
        catch (ServiceError $error) {
            if ($error->docUrl() !== $case['docUrl'] || $error->metadata->docUrl !== $case['docUrl']
                || (!($error instanceof UnknownServiceError)) !== $case['typed']
                || $error->metadata->status !== $case['status'] || $error->metadata->requestId !== 'documentation-consumer') {
                throw new RuntimeException($case['name'] . ': URL=' . ($error->docUrl() ?? 'null') . ', type=' . get_class($error));
            }
            $last = $error;
            if (in_array($case['name'], ['document-empty', 'document-whitespace', 'document-null'], true)) {
                $absent = $case['name'] !== 'document-null';
                if (($error->data === \Example\ErrorDocs\Runtime\Absent::Value) !== $absent
                    || (!$absent && $error->data !== null)) throw new RuntimeException($case['name'] . ': document presence');
            }
        }
    }
    $client->close();
    unset($client);
    if ($last->docUrl() !== $cases[array_key_last($cases)]['docUrl']) throw new RuntimeException('URL lifetime');
    $stderr = stream_get_contents($pipes[2]);
    fclose($pipes[1]); fclose($pipes[2]);
    $status = proc_close($process); $process = null;
    if ($status !== 0) throw new RuntimeException('Fixture failed: ' . $stderr);
} finally {
    if (is_resource($process)) {
        proc_terminate($process);
        foreach ($pipes as $pipe) if (is_resource($pipe)) fclose($pipe);
        proc_close($process);
    }
}
echo 'PHP error documentation: ', count($cases), " installed Composer/native HTTP cases passed\n";
