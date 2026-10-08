/// <summary>
/// Measures a client's response rate and, after its first throttling response, limits its send rate with CUBIC.
/// Times are seconds on the client's clock. Thread-safe; shared by every call of one client.
/// </summary>
public sealed class AdaptiveRateLimiter
{
    /// <summary>The slowest send rate, in requests per second.</summary>
    public const double MinFillRate = 0.5;
    /// <summary>The smallest token bucket size.</summary>
    public const double MinCapacity = 1;
    /// <summary>Weight of history in the smoothed measured rate.</summary>
    public const double Smoothing = 0.8;
    /// <summary>Width in seconds of one rate measurement bucket.</summary>
    public const double TimeBucketRange = 0.5;
    /// <summary>How far above the measured rate the fill rate may climb.</summary>
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

    /// <summary>Starts disabled; throttling responses enable it.</summary>
    /// <param name="now">The current monotonic time in seconds.</param>
    public AdaptiveRateLimiter(double now)
    {
        lastBucket = Math.Floor(now);
        cubic = new CubicCalculator(0, now);
    }
    /// <summary>The smoothed rate of responses, in requests per second.</summary>
    public double MeasuredRate { get { lock (gate) return measured; } }
    /// <summary>The current token refill rate, in requests per second.</summary>
    public double FillRate { get { lock (gate) return fillRate; } }
    /// <summary>Whether a throttling response has enabled rate limiting.</summary>
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
