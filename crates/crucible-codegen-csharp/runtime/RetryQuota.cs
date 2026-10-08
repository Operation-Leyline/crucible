/// <summary>A token bucket bounding how much of a client's traffic may be retries; shared by all of its calls.</summary>
public sealed class RetryQuota
{
    /// <summary>The size of a full bucket.</summary>
    public const int InitialTokens = 500;
    /// <summary>Tokens spent by one retry.</summary>
    public const int RetryCost = 5;
    /// <summary>Tokens spent by one retry after a timeout.</summary>
    public const int TimeoutRetryCost = 10;
    /// <summary>Tokens refunded by a call that never retried.</summary>
    public const int NoRetryIncrement = 1;
    private readonly object gate = new();
    private int available;

    /// <summary>A full bucket.</summary>
    public RetryQuota() : this(InitialTokens) { }
    /// <summary>A partly filled bucket.</summary>
    /// <param name="available">The starting tokens, from 0 to <see cref="InitialTokens"/>.</param>
    public RetryQuota(int available)
    {
        if (available < 0 || available > InitialTokens) throw new ArgumentOutOfRangeException(nameof(available));
        this.available = available;
    }
    /// <summary>The tokens currently available.</summary>
    public int Available { get { lock (gate) return available; } }

    /// <summary>The cost of one retry, or null (and nothing spent) when the quota cannot pay for it.</summary>
    public int? TryAcquire(bool timeout)
    {
        var cost = timeout ? TimeoutRetryCost : RetryCost;
        lock (gate)
        {
            if (cost > available) return null;
            available -= cost;
            return cost;
        }
    }

    /// <summary>A successful call refunds its last retry's cost, or adds a small increment when it never retried.</summary>
    public void Release(int? lastRetryCost)
    {
        lock (gate) available = Math.Min(InitialTokens, available + (lastRetryCost ?? NoRetryIncrement));
    }
}
