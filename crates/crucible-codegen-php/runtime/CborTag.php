<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** A tagged CBOR data item (major type 6), such as tag 1 epoch timestamps. */
final readonly class CborTag
{
    public function __construct(public int $tag, public mixed $value)
    {
        if ($tag < 0) throw new \InvalidArgumentException('CBOR tags are unsigned');
    }
}
