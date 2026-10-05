<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** The waiter's total deadline passed before a terminal acceptor matched. */
final class WaiterTimeoutException extends WaiterException
{
    public function __construct(string $waiter, int $attempts, mixed $response = null, ?\Throwable $error = null)
    {
        parent::__construct($waiter, $attempts, $response, $error, null, 'Waiter ' . $waiter . ' timed out after ' . $attempts . ' attempts');
    }
}
