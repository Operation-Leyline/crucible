<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * A response the service sent that is not a success.
 *
 * Catch the service exception class to handle every modeled and unknown
 * service error, or a generated error class for one modeled error. Transport
 * failures are a NetworkException or TimeoutException instead.
 */
abstract class ServiceError extends \RuntimeException
{
    /** @param ErrorMetadata $metadata status, request ID and retry classification of the failed response */
    public function __construct(public readonly ErrorMetadata $metadata = new ErrorMetadata())
    {
        parent::__construct($metadata->message);
    }
}
