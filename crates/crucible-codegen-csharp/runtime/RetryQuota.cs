/// <summary>A token bucket bounding how much of a client's traffic may be retries; shared by all of its calls.</summary>
public sealed class RetryQuota
{
    public const int InitialTokens = 500;
    public const int RetryCost = 5;
    public const int TimeoutRetryCost = 10;
    public const int NoRetryIncrement = 1;
    private readonly object gate = new();
    private int available;

    public RetryQuota() : this(InitialTokens) { }
    public RetryQuota(int available)
    {
        if (available < 0 || available > InitialTokens) throw new ArgumentOutOfRangeException(nameof(available));
        this.available = available;
    }
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
