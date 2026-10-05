<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** Exact arbitrary-precision integer, represented without floating-point conversion. */
final readonly class BigInteger implements \Stringable
{
    public JsonNumber $number;
    public function __construct(string $value)
    {
        $this->number = new JsonNumber($value);
        if (str_starts_with($this->number->canonical()[1], '-')) throw new \UnexpectedValueException('Expected integer');
    }
    public function __toString(): string { return $this->number->text; }
    public function equals(self $other): bool { return $this->number->canonical() === $other->number->canonical(); }
}
