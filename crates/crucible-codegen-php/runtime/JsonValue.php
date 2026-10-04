<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** An immutable JSON document; object values are defensively copied. */
final readonly class JsonValue
{
    private string $source;
    public function __construct(mixed $value) { $this->source = ExactJson::encode($value); }
    public static function parse(string $source): self { return new self(ExactJson::decode($source)); }
    public function value(): mixed { return ExactJson::decode($this->source); }
    public function toJson(): string { return $this->source; }
}
