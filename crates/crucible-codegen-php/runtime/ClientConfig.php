<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class ClientConfig
{
    /**
     * @param array<string, string> $headers @param list<\Closure(HttpRequest): HttpRequest> $middleware
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
