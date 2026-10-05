/// <summary>
/// Measures a client's response rate and, after its first throttling response, limits its send rate with CUBIC.
/// Times are seconds on the client's clock. Thread-safe; shared by every call of one client.
/// </summary>
public sealed class AdaptiveRateLimiter
{
    public const double MinFillRate = 0.5;
    public const double MinCapacity = 1;
    public const double Smoothing = 0.8;
    public const double TimeBucketRange = 0.5;
    public const double MaxRateAdjustScale = 2.0;
    private readonly object gate = new();
    private readonly CubicCalculator cubic;
    // The smoothed rate of responses, measured in half-second buckets.
    private double measured;
    private int count;
    private double lastBucket;
    // The send token bucket, disabled until the first throttle.
    private double fillRate = MinFillRate;
    private double maxCapacity = MinCapacity;
    private double capacity;
    private double? last;
    private bool enabled;

    public AdaptiveRateLimiter(double now)
    {
        lastBucket = Math.Floor(now);
        cubic = new CubicCalculator(0, now);
    }
    public double MeasuredRate { get { lock (gate) return measured; } }
    public double FillRate { get { lock (gate) return fillRate; } }
    public bool Enabled { get { lock (gate) return enabled; } }

    /// <summary>Takes one send token, possibly on credit, and returns how many seconds to wait before sending.</summary>
    public double BeforeSend(double now)
    {
        lock (gate)
        {
            if (!enabled) return 0;
            Refill(now);
            capacity -= 1;
            return capacity >= 0 ? 0 : -capacity / fillRate;
        }
    }

    /// <summary>Returns the token of a send that was cancelled while waiting.</summary>
    public void ReturnToken()
    {
        lock (gate) if (enabled) capacity = Math.Min(maxCapacity, capacity + 1);
    }

    /// <summary>Records an attempt that reached the transport; only a modeled throttling error counts as throttled.</summary>
    public void AfterAttempt(bool throttled, double now)
    {
        lock (gate)
        {
            var bucket = Math.Floor(now * (1 / TimeBucketRange)) / (1 / TimeBucketRange);
            count++;
            if (bucket > lastBucket)
            {
                measured = count / (bucket - lastBucket) * Smoothing + measured * (1 - Smoothing);
                count = 0;
                lastBucket = bucket;
            }
            double rate;
            if (throttled)
            {
                rate = cubic.Throttle(enabled ? Math.Min(measured, fillRate) : measured, now);
                enabled = true;
            }
            else rate = cubic.Success(now);
            rate = Math.Min(rate, MaxRateAdjustScale * measured);
            // Tokens earned at the old rate are credited before the rate changes.
            Refill(now);
            fillRate = Math.Max(rate, MinFillRate);
            maxCapacity = Math.Max(rate, MinCapacity);
            capacity = Math.Min(capacity, maxCapacity);
        }
    }

    private void Refill(double now)
    {
        if (last is double previous) capacity = Math.Min(maxCapacity, capacity + (now - previous) * fillRate);
        last = now;
    }
}
