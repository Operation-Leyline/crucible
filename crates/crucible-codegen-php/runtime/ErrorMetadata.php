<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class ErrorMetadata
{
    /** @param array<string, string> $headers */
    public function __construct(
        public string $name = 'UnknownError', public string $message = '',
        public int $status = 0, public ?string $requestId = null,
        public array $headers = [], public bool $retryable = false, public bool $throttling = false,
    ) {}
}
