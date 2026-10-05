<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class ClientConfig
{
    /** @param array<string, string> $headers @param list<\Closure(HttpRequest): HttpRequest> $middleware */
    public function __construct(
        public ?string $endpoint = null, public ?\Closure $token = null,
        public ?HttpTransport $transport = null, public float $timeout = 30,
        public int $maxAttempts = 3, public float $baseDelay = 0.1, public float $maxDelay = 10,
        public array $headers = [], public array $middleware = [],
        public ?\Closure $basicAuth = null, public ?\Closure $apiKey = null,
        public ?\Closure $credentials = null, public ?string $region = null,
    ) {
        if (!is_finite($timeout) || $timeout <= 0 || $maxAttempts < 1 || !is_finite($baseDelay) || $baseDelay < 0 || !is_finite($maxDelay) || $maxDelay < 0) {
            throw new \InvalidArgumentException('Invalid timeout, retry delay, or attempt limit');
        }
    }
}
