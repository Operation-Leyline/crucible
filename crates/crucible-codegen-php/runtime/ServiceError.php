<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

abstract class ServiceError extends \RuntimeException
{
    public function __construct(public readonly ErrorMetadata $metadata = new ErrorMetadata())
    {
        parent::__construct($metadata->message);
    }
}
