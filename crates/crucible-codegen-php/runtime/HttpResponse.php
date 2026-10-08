<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** One HTTP response with lower-cased header names. */
final readonly class HttpResponse
{
    /** @var array<string, string> the response headers, names in lower case */
    public array $headers;
    /**
     * @param int $status the status code
     * @param array<string, string> $headers the response headers
     * @param string $body the body bytes; empty when none was read
     */
    public function __construct(public int $status, array $headers = [], public string $body = '')
    {
        $normalized = [];
        ksort($headers, SORT_STRING);
        foreach ($headers as $name => $value) $normalized[strtolower($name)] = $value;
        $this->headers = $normalized;
    }
}
