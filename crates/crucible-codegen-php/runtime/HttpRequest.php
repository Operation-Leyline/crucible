<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class HttpRequest
{
    /** @param array<string, string> $headers */
    public function __construct(public string $url, public string $method, public array $headers = [], public ?string $body = null) {}
    /** @param array<string, string> $headers */
    public function withHeaders(array $headers): self { return new self($this->url, $this->method, $headers, $this->body); }
    public function withBody(?string $body): self { return new self($this->url, $this->method, $this->headers, $body); }
}
