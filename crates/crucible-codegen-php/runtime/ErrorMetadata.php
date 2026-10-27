<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** What the transport learned about a failed call, apart from the modeled body. */
final readonly class ErrorMetadata
{
    /**
     * @param string $name the error's shape name as received, or UnknownError when none was sent
     * @param string $message the service's human-readable message, empty when it sent none
     * @param int $status the HTTP status code, or 0 when there was no HTTP response
     * @param ?string $requestId the request ID from the response headers, when the service sent one
     * @param array<string, string> $headers the response headers with lower-case names
     * @param bool $retryable whether the retry policy treats this failure as transient
     * @param bool $throttling whether the service asked the client to slow down
     */
    public function __construct(
        public string $name = 'UnknownError', public string $message = '',
        public int $status = 0, public ?string $requestId = null,
        public array $headers = [], public bool $retryable = false, public bool $throttling = false,
        // BEGIN ERROR DOCUMENTATION
        /** Static modeled code/reason page, or null for an unknown error identity. */
        public ?string $docUrl = null,
        // END ERROR DOCUMENTATION
    ) {}

    /** Keeps var_dump() and print_r() from listing $message and $headers, which may carry sensitive data. */
    public function __debugInfo(): array
    {
        return ['name' => $this->name, 'status' => $this->status, 'requestId' => $this->requestId,
            'retryable' => $this->retryable, 'throttling' => $this->throttling];
    }

    /** Omits $message and $headers: they may carry sensitive data. */
    public function __toString(): string
    {
        return 'ErrorMetadata(name=' . $this->name . ', status=' . $this->status
            . ($this->requestId === null ? '' : ', requestId=' . $this->requestId)
            . ($this->retryable ? ', retryable=true' : '') . ($this->throttling ? ', throttling=true' : '') . ')';
    }
}
