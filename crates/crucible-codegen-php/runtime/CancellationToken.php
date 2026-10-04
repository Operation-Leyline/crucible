<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final class CancellationToken
{
    private bool $cancelled = false;
    /** A polling predicate lets a synchronous caller observe external cancellation. */
    public function __construct(private readonly ?\Closure $requested = null) {}
    public function cancel(): void { $this->cancelled = true; }
    public function check(): void
    {
        if ($this->cancelled || ($this->requested !== null && ($this->requested)())) {
            $this->cancelled = true;
            throw new CancelledException('Request cancelled');
        }
    }
}
