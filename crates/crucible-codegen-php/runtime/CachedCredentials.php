<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

/**
 * Shares one provider across calls and refreshes expiring identities once,
 * before they expire. Usable directly as a ClientConfig credentials provider.
 */
final class CachedCredentials
{
    private ?AwsCredentials $value = null;
    private ?float $refreshAt = null;
    /**
     * @param \Closure(): ?AwsCredentials $provider the wrapped source, called only when the cached value is missing or due
     * @param float $refreshBefore seconds before expiry to refresh, at most half the remaining lifetime
     * @param ?\Closure(): float $clock the time source in seconds, replaceable in tests
     */
    public function __construct(private readonly \Closure $provider, private readonly float $refreshBefore = 60,
        private readonly ?\Closure $clock = null)
    {
        if (!is_finite($refreshBefore) || $refreshBefore < 0) throw new \InvalidArgumentException('Invalid credential refresh interval');
    }
    /** Returns the cached credentials, refreshing them first when due. */
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
