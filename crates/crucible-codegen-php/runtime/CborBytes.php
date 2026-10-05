<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** A CBOR byte string (major type 2), distinct from a UTF-8 text string. */
final readonly class CborBytes
{
    public function __construct(public string $bytes) {}
}
