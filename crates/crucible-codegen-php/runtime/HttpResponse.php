<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class HttpResponse
{
    public array $headers;
    /** @param array<string, string> $headers */
    public function __construct(public int $status, array $headers = [], public string $body = '')
    {
        $normalized = [];
        ksort($headers, SORT_STRING);
        foreach ($headers as $name => $value) $normalized[strtolower($name)] = $value;
        $this->headers = $normalized;
    }
}
