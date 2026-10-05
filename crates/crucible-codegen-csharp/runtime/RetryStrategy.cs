/// <summary>One client's retry state: its mode, quota and rate limiter, shared by all of its calls.</summary>
internal sealed class RetryStrategy
{
    private readonly TimeProvider clock;
    private readonly TimeSpan baseDelay;
    private readonly TimeSpan maxDelay;
    internal Func<double> Jitter = Random.Shared.NextDouble;
    internal RetryQuota? Quota { get; }
    internal AdaptiveRateLimiter? Limiter { get; }

    internal RetryStrategy(RetryMode mode, TimeProvider clock, TimeSpan baseDelay, TimeSpan maxDelay)
    {
        this.clock = clock; this.baseDelay = baseDelay; this.maxDelay = maxDelay;
        if (mode != RetryMode.Legacy) Quota = new RetryQuota();
        if (mode == RetryMode.Adaptive) Limiter = new AdaptiveRateLimiter(Now());
    }
    /// <summary>Seconds on the client's monotonic clock.</summary>
    internal double Now() => clock.GetTimestamp() / (double)clock.TimestampFrequency;
    internal TimeSpan SendDelay() => Limiter is null ? TimeSpan.Zero : TimeSpan.FromSeconds(Limiter.BeforeSend(Now()));
    internal void CancelSend() => Limiter?.ReturnToken();
    internal void AfterAttempt(bool throttled) => Limiter?.AfterAttempt(throttled, Now());
    /// <summary>Whether a retry may proceed, and the quota it consumed.</summary>
    internal (bool Granted, int? Cost) AcquireRetry(bool timeout)
    {
        if (Quota is null) return (true, null);
        var cost = Quota.TryAcquire(timeout);
        return (cost is not null, cost);
    }
    internal void Succeeded(int? lastRetryCost) => Quota?.Release(lastRetryCost);
    /// <summary>Full-jitter exponential backoff after the given (1-based) failed attempt.</summary>
    internal TimeSpan Backoff(int attempt) =>
        TimeSpan.FromMilliseconds(Jitter() * Math.Min(maxDelay.TotalMilliseconds, baseDelay.TotalMilliseconds * Math.Pow(2, Math.Min(attempt - 1, 30))));
}
