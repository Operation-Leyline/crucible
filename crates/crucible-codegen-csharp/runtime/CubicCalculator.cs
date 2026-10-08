/// <summary>CUBIC congestion control applied to a client's send rate, as in the AWS SDKs' adaptive retry mode.</summary>
public sealed class CubicCalculator
{
    /// <summary>Multiplicative decrease applied to the rate on throttling.</summary>
    public const double Beta = 0.7;
    /// <summary>The CUBIC growth scale.</summary>
    public const double ScaleConstant = 0.4;
    private double lastMaxRate;
    private double k;
    private double lastThrottle;

    /// <summary>Starts a CUBIC curve.</summary>
    /// <param name="lastMaxRate">The rate at the last throttle.</param>
    /// <param name="lastThrottleTime">When the last throttle happened, in seconds.</param>
    public CubicCalculator(double lastMaxRate, double lastThrottleTime)
    {
        this.lastMaxRate = lastMaxRate;
        k = ZeroPoint();
        lastThrottle = lastThrottleTime;
    }
    private double ZeroPoint() => Math.Cbrt(lastMaxRate * (1 - Beta) / ScaleConstant);

    /// <summary>The rate after a response that was not throttled, at <paramref name="time"/> seconds.</summary>
    public double Success(double time)
    {
        var x = time - lastThrottle - k;
        return ScaleConstant * x * x * x + lastMaxRate;
    }

    /// <summary>Records a throttle at <paramref name="rate"/> and returns the reduced rate.</summary>
    public double Throttle(double rate, double time)
    {
        lastMaxRate = rate;
        k = ZeroPoint();
        lastThrottle = time;
        return rate * Beta;
    }
}
