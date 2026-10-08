<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** The empty input or output of an operation that takes or returns nothing. */
final readonly class Unit implements WireModel
{
    public function __toString(): string { return 'Unit()'; }
    public function toWire(WireContext $context): JsonObject { return new JsonObject(); }
}
