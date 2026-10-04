<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class CallScope
{
    private int $start;
    public function __construct(private float $timeout, private ?CancellationToken $cancellation)
    {
        $this->start = hrtime(true);
    }
    public function remaining(): float { return max(0.0, $this->timeout - (hrtime(true) - $this->start) / 1e9); }
    public function check(): void
    {
        $this->cancellation?->check();
        if ($this->remaining() <= 0) throw new TimeoutException('Request deadline exceeded');
    }
    public function sleep(float $seconds): void
    {
        $start = hrtime(true);
        do {
            $this->check();
            $remaining = $seconds - (hrtime(true) - $start) / 1e9;
            if ($remaining <= 0) return;
            usleep((int)max(1, min(0.05, $remaining, $this->remaining()) * 1e6));
        } while (true);
    }
}
