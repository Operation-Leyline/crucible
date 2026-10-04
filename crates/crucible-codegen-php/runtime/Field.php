<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** @template T */
final readonly class Field
{
    /** @param T|null $value */
    public function __construct(public bool $isSet = false, public mixed $value = null) {}
    /** @template V @param V $value @return Field<V> */
    public static function value(mixed $value): self { return new self(true, $value); }
    /** @return Field<null> */
    public static function nullValue(): self { return new self(true, null); }
}
