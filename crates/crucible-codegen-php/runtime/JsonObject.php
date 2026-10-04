<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** JSON object identity, including empty objects and keys beginning with NUL. */
final readonly class JsonObject
{
    /** @param array<string|int, mixed> $members */
    public function __construct(public array $members = []) {}
    public function has(string $key): bool { return array_key_exists($key, $this->members); }
    public function get(string $key): mixed { return $this->members[$key] ?? null; }
}
