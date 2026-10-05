<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** Exact arbitrary-precision decimal, including its original wire precision. */
final readonly class BigDecimal implements \Stringable
{
    public JsonNumber $number;
    public function __construct(string $value) { $this->number = new JsonNumber($value); }
    public function __toString(): string { return $this->number->text; }
    public function equals(self $other): bool { return $this->number->canonical() === $other->number->canonical(); }
}
