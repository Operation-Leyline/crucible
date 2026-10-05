<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class HttpRequest
{
    /**
     * A streaming body is $stream, opened by the transport for each attempt, and $body is null.
     * @param array<string, string> $headers
     */
    public function __construct(public string $url, public string $method, public array $headers = [], public ?string $body = null, public ?ByteStream $stream = null) {}
    /** @param array<string, string> $headers */
    public function withHeaders(array $headers): self { return new self($this->url, $this->method, $headers, $this->body, $this->stream); }
    /** Replaces the body with bytes, dropping any stream. */
    public function withBody(?string $body): self { return new self($this->url, $this->method, $this->headers, $body); }
    public function withStream(?ByteStream $stream): self { return new self($this->url, $this->method, $this->headers, null, $stream); }
}
