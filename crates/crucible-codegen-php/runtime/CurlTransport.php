<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** A reusable connection pool, without redirect following, response caching, or a cookie jar. */
final class CurlTransport implements HttpTransport
{
    private \CurlMultiHandle $multi;
    private bool $closed = false;
    private bool $active = false;
    public function __construct() { $this->multi = curl_multi_init(); }
    public function close(): void
    {
        if (!$this->closed) { curl_multi_close($this->multi); $this->closed = true; }
    }
    /** @param ?\Closure(string, array<string, string>): void $sink receives a 2xx body chunk by chunk */
    public function send(HttpRequest $request, bool $readBody, CallScope $scope, ?\Closure $sink = null): HttpResponse
    {
        $scope->check();
        if ($this->closed) throw new \LogicException('Transport is closed');
        if ($this->active) throw new \LogicException('Transport cannot send reentrantly');
        $this->active = true;
        $easy = curl_init($request->url);
        $headers = [];
        $status = 0;
        $body = '';
        $discarded = false;
        $added = false;
        $upload = null;
        // A callback cannot throw through libcurl: it records the failure and pauses or aborts.
        $failure = null;
        try {
            $requestHeaders = [];
            foreach ($request->headers as $name => $value) {
                if (!preg_match('/^[!#$%&\'*+.^_`|~0-9a-z-]+$/iD', $name) || strpbrk($value, "\r\n\0") !== false) {
                    throw new \InvalidArgumentException('Invalid HTTP header');
                }
                $requestHeaders[] = $value === '' ? $name . ';' : $name . ': ' . $value;
            }
            if ($request->stream !== null) {
                $upload = $request->stream->open();
                // Sending the body at once avoids a 100-continue round trip per request.
                $requestHeaders[] = 'Expect:';
            }
            $milliseconds = ceil($scope->remaining() * 1000);
            curl_setopt_array($easy, [
                CURLOPT_CUSTOMREQUEST => $request->method,
                CURLOPT_HTTPHEADER => $requestHeaders,
                CURLOPT_FOLLOWLOCATION => false,
                CURLOPT_PROTOCOLS => CURLPROTO_HTTP | CURLPROTO_HTTPS,
                CURLOPT_NOSIGNAL => true,
                CURLOPT_SUPPRESS_CONNECT_HEADERS => true,
                CURLOPT_TIMEOUT_MS => $milliseconds <= 2147483647 ? max(1, (int)$milliseconds) : 0,
                CURLOPT_HEADERFUNCTION => static function ($handle, string $line) use (&$headers, &$status, &$discarded, $readBody): int {
                    if (preg_match('/^HTTP\/\S+\s+(\d{3})\b/', $line, $match)) { $status = (int)$match[1]; $headers = []; }
                    elseif (trim($line) === '') {
                        if ($status >= 200 && $status < 300 && !$readBody) { $discarded = true; return 0; }
                    } elseif (str_contains($line, ':')) {
                        [$name, $value] = explode(':', $line, 2);
                        $name = strtolower(trim($name));
                        $value = trim($value);
                        $headers[$name] = isset($headers[$name]) ? $headers[$name] . ', ' . $value : $value;
                    }
                    return strlen($line);
                },
                CURLOPT_BUFFERSIZE => 131072,
                CURLOPT_WRITEFUNCTION => static function ($handle, string $chunk) use (&$body, &$status, &$headers, &$failure, $sink): int {
                    if ($sink === null || $status < 200 || $status >= 300) { $body .= $chunk; return strlen($chunk); }
                    try { $sink($chunk, $headers); return strlen($chunk); }
                    catch (\Throwable $error) { $failure ??= $error; return 0; }
                },
            ]);
            if ($upload !== null) {
                curl_setopt_array($easy, [
                    CURLOPT_UPLOAD => true,
                    CURLOPT_CUSTOMREQUEST => $request->method,
                    CURLOPT_READFUNCTION => static function ($handle, $file, int $size) use ($upload, $scope, &$failure): string|int {
                        if ($failure !== null) return CURL_READFUNC_PAUSE;
                        try { $scope->check(); return $upload->read($size); }
                        // Pausing keeps a chunked upload from ending as if it were complete.
                        catch (\Throwable $error) { $failure = $error; return CURL_READFUNC_PAUSE; }
                    },
                ]);
                if ($request->stream->length !== null) curl_setopt($easy, CURLOPT_INFILESIZE, $request->stream->length);
            } elseif ($request->body !== null) curl_setopt($easy, CURLOPT_POSTFIELDS, $request->body);
            curl_multi_add_handle($this->multi, $easy);
            $added = true;
            do {
                $scope->check();
                if ($this->closed) throw new \LogicException('Transport is closed');
                $result = curl_multi_exec($this->multi, $running);
                if ($failure !== null) throw $failure;
                if ($result !== CURLM_OK) throw new NetworkException(curl_multi_strerror($result));
                if (!$running) break;
                if (curl_multi_select($this->multi, min(0.05, $scope->remaining())) === -1) usleep(1000);
            } while (true);
            if ($failure !== null) throw $failure;
            $scope->check();
            $error = null;
            while (($completed = curl_multi_info_read($this->multi)) !== false) {
                if ($completed['handle'] === $easy && $completed['msg'] === CURLMSG_DONE) $error = $completed['result'];
            }
            if ($error === null) throw new NetworkException('Missing transfer completion');
            if ($error !== CURLE_OK && !($discarded && $error === CURLE_WRITE_ERROR)) {
                if ($error === CURLE_OPERATION_TIMEDOUT) throw new TimeoutException('Request deadline exceeded');
                throw new NetworkException(curl_error($easy), $error);
            }
            if ($status === 0) throw new NetworkException('No HTTP response');
            return new HttpResponse($status, $headers, $body);
        } finally {
            if ($added && !$this->closed) curl_multi_remove_handle($this->multi, $easy);
            // Releasing the last reference closes a CurlHandle on supported PHP versions.
            unset($easy);
            $upload?->close();
            $this->active = false;
        }
    }
}
