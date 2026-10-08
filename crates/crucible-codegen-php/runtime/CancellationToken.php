<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * Caller-owned cancellation shared by any number of calls.
 *
 * Pass it as the cancellation argument of an operation; cancelling it fails
 * the call with CancelledException.
 */
final class CancellationToken
{
    private bool $cancelled = false;
    /** @param ?\Closure(): bool $requested a polling predicate lets a synchronous caller observe external cancellation */
    public function __construct(private readonly ?\Closure $requested = null) {}
    /** Cancels every call that uses this token. */
    public function cancel(): void { $this->cancelled = true; }
    /** @throws CancelledException when cancelled */
    public function check(): void
    {
        if ($this->cancelled || ($this->requested !== null && ($this->requested)())) {
            $this->cancelled = true;
            throw new CancelledException('Request cancelled');
        }
    }
}
