<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * Settings for a generated client, fixed when the client is constructed.
 *
 * Every setting is optional. Without credentials, only operations that need
 * none can succeed. A configuration is immutable and may be shared by any
 * number of clients.
 */
final readonly class ClientConfig
{
    /**
     * @param ?string $endpoint the service base URL, overriding the one generated into the client; absolute HTTP(S) without credentials, query or fragment
     * @param ?\Closure(): ?string $token bearer token source, for services that use HTTP bearer authentication; null means none
     * @param ?HttpTransport $transport sends the HTTP requests; defaults to a cURL transport that the client owns and closes with itself, while one you supply stays yours to close
     * @param float $timeout seconds one call may take in total, covering every attempt and retry delay; exceeding it throws TimeoutException
     * @param int $maxAttempts the most times a request is sent, the first attempt included; 1 disables retries. Only read-only or idempotent operations are retried after a network failure or a 500, 502, 503, 504 or retryable response; throttling responses are retried for any operation
     * @param float $baseDelay ceiling in seconds of the first retry delay; delays use exponential backoff with full jitter, doubling per attempt up to $maxDelay, and a Retry-After response header takes precedence
     * @param float $maxDelay ceiling in seconds of any single backoff delay
     * @param array<string, string> $headers headers added to every request
     * @param list<\Closure(HttpRequest): HttpRequest> $middleware request rewriters, run in order on every attempt
     * @throws \InvalidArgumentException when a timeout, delay or attempt limit is invalid
     */
    public function __construct(
        public ?string $endpoint = null, public ?\Closure $token = null,
        public ?HttpTransport $transport = null, public float $timeout = 30,
        public int $maxAttempts = 3, public float $baseDelay = 0.1, public float $maxDelay = 10,
        public array $headers = [], public array $middleware = [],
    ) {
        if (!is_finite($timeout) || $timeout <= 0 || $maxAttempts < 1 || !is_finite($baseDelay) || $baseDelay < 0 || !is_finite($maxDelay) || $maxDelay < 0) {
            throw new \InvalidArgumentException('Invalid timeout, retry delay, or attempt limit');
        }
    }
}
