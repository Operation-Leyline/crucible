<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/** A waiter reached a failure acceptor, or an error that no acceptor matched. */
class WaiterException extends \RuntimeException
{
    /** @param string|list<string>|null $reason the modeled acceptor message */
    public function __construct(
        public readonly string $waiter, public readonly int $attempts,
        public readonly mixed $response = null, ?\Throwable $error = null,
        public readonly string|array|null $reason = null, ?string $message = null,
    ) {
        $detail = is_array($reason) ? implode(', ', $reason) : $reason;
        parent::__construct($message ?? ('Waiter ' . $waiter . ' failed after ' . $attempts . ' attempts' . ($detail === null ? '' : ': ' . $detail)), 0, $error);
    }
}
