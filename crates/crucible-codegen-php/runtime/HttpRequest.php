<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** One outgoing HTTP request. Immutable: the with* methods return changed copies. */
final readonly class HttpRequest
{
    /**
     * A streaming body is $stream, opened by the transport for each attempt, and $body is null.
     * @param string $url the absolute URL
     * @param string $method the HTTP method, upper case
     * @param array<string, string> $headers the request headers
     * @param ?string $body the body bytes, or null for none
     * @param ?ByteStream $stream a streaming body, in place of $body
     */
    public function __construct(public string $url, public string $method, public array $headers = [], public ?string $body = null, public ?ByteStream $stream = null) {}
    /** @param array<string, string> $headers */
    public function withHeaders(array $headers): self { return new self($this->url, $this->method, $headers, $this->body, $this->stream); }
    /** Replaces the body with bytes, dropping any stream. */
    public function withBody(?string $body): self { return new self($this->url, $this->method, $this->headers, $body); }
    /** Replaces the body with a stream, dropping any bytes. */
    public function withStream(?ByteStream $stream): self { return new self($this->url, $this->method, $this->headers, null, $stream); }
}
