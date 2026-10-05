import kotlin.math.cbrt
import kotlin.math.pow

/** CUBIC congestion control applied to a client's send rate. Times are seconds on the client's [RuntimeClock]. */
class CubicCalculator(lastMaxRate: Double, lastThrottleTime: Double) {
    var lastMaxRate: Double = lastMaxRate
        private set
    var lastThrottleTime: Double = lastThrottleTime
        private set
    private var k = zeroPoint()

    private fun zeroPoint(): Double = cbrt(lastMaxRate * (1 - AdaptiveRateLimiter.BETA) / AdaptiveRateLimiter.SCALE_CONSTANT)

    /** The rate CUBIC allows at [now] since the last throttle. */
    fun success(now: Double): Double = AdaptiveRateLimiter.SCALE_CONSTANT * (now - lastThrottleTime - k).pow(3) + lastMaxRate

    /** Records a throttle at [now] while sending at [rate]; returns the reduced rate. */
    fun throttle(rate: Double, now: Double): Double {
        lastMaxRate = rate
        k = zeroPoint()
        lastThrottleTime = now
        return rate * AdaptiveRateLimiter.BETA
    }
}
