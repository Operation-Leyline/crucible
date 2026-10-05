<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final class CachedCredentials
{
    private ?AwsCredentials $value = null;
    private ?float $refreshAt = null;
    public function __construct(private readonly \Closure $provider, private readonly float $refreshBefore = 60,
        private readonly ?\Closure $clock = null)
    {
        if (!is_finite($refreshBefore) || $refreshBefore < 0) throw new \InvalidArgumentException('Invalid credential refresh interval');
    }
    public function __invoke(): ?AwsCredentials
    {
        $now = $this->clock === null ? microtime(true) : ($this->clock)();
        if ($this->value !== null && ($this->refreshAt === null || $this->refreshAt > $now)) return $this->value;
        $value = ($this->provider)();
        if ($value !== null && !$value instanceof AwsCredentials) throw new \UnexpectedValueException('Expected AwsCredentials');
        $this->refreshAt = null;
        if ($value?->expiresAt !== null) {
            $remaining = (float)$value->expiresAt->format('U.u') - $now;
            if ($remaining <= 0) throw new \UnexpectedValueException('Credential provider returned expired credentials');
            $this->refreshAt = $now + $remaining - min($this->refreshBefore, $remaining / 2);
        }
        return $this->value = $value;
    }
    public function __debugInfo(): array { return ['credentials'=>'[redacted]']; }
}
