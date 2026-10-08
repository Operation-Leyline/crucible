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
     * @param ?string $endpoint the service base URL, overriding the one generated into the client; absolute HTTP(S) without credentials, query or fragment, and required unless the model's endpoint rules resolve every request
     * @param ?\Closure(): ?string $token bearer token source, for services that use HTTP bearer authentication; null means none
     * @param ?HttpTransport $transport sends the HTTP requests; defaults to a cURL transport that the client owns and closes with itself, while one you supply stays yours to close
     * @param float $timeout seconds one call may take in total, covering every attempt and retry delay; exceeding it throws TimeoutException
     * @param int $maxAttempts the most times a request is sent, the first attempt included; 1 disables retries. Only read-only or idempotent operations are retried after a network failure or a 500, 502, 503, 504 or retryable response; throttling responses are retried for any operation
     * @param float $baseDelay ceiling in seconds of the first retry delay; delays use exponential backoff with full jitter, doubling per attempt up to $maxDelay, and a Retry-After response header takes precedence
     * @param float $maxDelay ceiling in seconds of any single backoff delay
     * @param array<string, string> $headers headers added to every request
     * @param list<\Closure(HttpRequest): HttpRequest> $middleware request rewriters, run in order on every attempt
     * @param ?\Closure(): ?BasicCredentials $basicAuth HTTP Basic credential source
     * @param ?\Closure(): ?string $apiKey API key source, for services that use API key authentication
     * @param ?\Closure(): ?AwsCredentials $credentials AWS credential source for Signature Version 4; see Credentials and CachedCredentials
     * @param ?string $region the SigV4 signing region and the AWS::Region endpoint built-in
     * @param bool $disableHostPrefix stops modeled hostPrefix values from being prepended to the host, for example when $endpoint points at a local test server
     * @param string $requestChecksumCalculation Checksums::WHEN_SUPPORTED (the default) to checksum request payloads whenever the service supports it, or Checksums::WHEN_REQUIRED
     * @param string $responseChecksumValidation Checksums::WHEN_SUPPORTED (the default) to validate every response checksum the service sends, or Checksums::WHEN_REQUIRED
     * @throws \InvalidArgumentException when a timeout, delay, attempt limit, checksum policy, region set or endpoint parameter is invalid
     * @param array<string, string|bool|list<string>> $endpointParameters client context for modeled endpoint rules
     * @param array<string, string|bool|list<string>> $endpointBuiltIns built-in values, such as SDK::Endpoint, that override region and endpoint
     * @param ?\Closure(string, string): void $checksumValidated receives the operation and algorithm of each validated response checksum
     * @param ?\Closure(): ?BasicCredentials $digestAuth credentials for HTTP Digest challenges
     * @param ?list<string> $sigv4aSigningRegionSet regions a SigV4a signature covers, such as ['*']; defaults to [$region]
     */
    public function __construct(
        public ?string $endpoint = null, public ?\Closure $token = null,
        public ?HttpTransport $transport = null, public float $timeout = 30,
        public int $maxAttempts = 3, public float $baseDelay = 0.1, public float $maxDelay = 10,
        public array $headers = [], public array $middleware = [],
        public ?\Closure $basicAuth = null, public ?\Closure $apiKey = null,
        public ?\Closure $credentials = null, public ?string $region = null,
        public array $endpointParameters = [], public array $endpointBuiltIns = [], public bool $disableHostPrefix = false,
        public string $requestChecksumCalculation = Checksums::WHEN_SUPPORTED,
        public string $responseChecksumValidation = Checksums::WHEN_SUPPORTED,
        public ?\Closure $checksumValidated = null,
        public ?\Closure $digestAuth = null, public ?array $sigv4aSigningRegionSet = null,
    ) {
        if (!is_finite($timeout) || $timeout <= 0 || $maxAttempts < 1 || !is_finite($baseDelay) || $baseDelay < 0 || !is_finite($maxDelay) || $maxDelay < 0) {
            throw new \InvalidArgumentException('Invalid timeout, retry delay, or attempt limit');
        }
        foreach ([$requestChecksumCalculation, $responseChecksumValidation] as $policy) {
            if (!in_array($policy, [Checksums::WHEN_SUPPORTED, Checksums::WHEN_REQUIRED], true)) throw new \InvalidArgumentException('Checksum policies must be WHEN_SUPPORTED or WHEN_REQUIRED');
        }
        if ($sigv4aSigningRegionSet !== null && ($sigv4aSigningRegionSet === [] || !array_is_list($sigv4aSigningRegionSet) || array_filter($sigv4aSigningRegionSet, static fn($region) => !is_string($region) || $region === '' || strpbrk($region, ",/\r\n") !== false) !== [])) {
            throw new \InvalidArgumentException('The SigV4a signing region set must be a nonempty list of regions');
        }
        foreach ([...array_values($endpointParameters), ...array_values($endpointBuiltIns)] as $value) {
            if (!is_string($value) && !is_bool($value) && !(is_array($value) && array_is_list($value) && array_filter($value, static fn($item) => !is_string($item)) === [])) {
                throw new \InvalidArgumentException('Endpoint parameters must be strings, booleans, or lists of strings');
            }
        }
    }
}
