<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** One outgoing HTTP request. Immutable: the with* methods return changed copies. */
final readonly class HttpRequest
{
    /**
     * @param string $url the absolute URL
     * @param string $method the HTTP method, upper case
     * @param array<string, string> $headers the request headers
     * @param ?string $body the body bytes, or null for none
     */
    public function __construct(public string $url, public string $method, public array $headers = [], public ?string $body = null) {}
    /** @param array<string, string> $headers */
    public function withHeaders(array $headers): self { return new self($this->url, $this->method, $headers, $this->body); }
    /** Replaces the body with bytes. */
    public function withBody(?string $body): self { return new self($this->url, $this->method, $this->headers, $body); }
}
