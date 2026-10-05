/// <summary>CUBIC congestion control applied to a client's send rate, as in the AWS SDKs' adaptive retry mode.</summary>
public sealed class CubicCalculator
{
    public const double Beta = 0.7;
    public const double ScaleConstant = 0.4;
    private double lastMaxRate;
    private double k;
    private double lastThrottle;

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
