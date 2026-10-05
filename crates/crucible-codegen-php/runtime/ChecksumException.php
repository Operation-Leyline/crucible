<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** A response checksum differs from the received payload. */
final class ChecksumException extends \UnexpectedValueException
{
    public function __construct(public readonly string $algorithm)
    {
        parent::__construct('Response ' . $algorithm . ' checksum mismatch');
    }
}
