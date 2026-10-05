import kotlin.math.floor

/**
 * The [RetryMode.ADAPTIVE] client-side rate limiter: it measures the rate responses arrive and,
 * after the first throttling response, limits the send rate with CUBIC. Times are seconds on the
 * client's [RuntimeClock], starting at [start]. Not thread-safe: a client serializes its use.
 */
class AdaptiveRateLimiter(start: Double) {
    /** The smoothed rate responses were received, in requests per second. */
    var measuredRate: Double = 0.0
        private set
    private var count = 0
    private var lastBucket = floor(start)

    val cubic: CubicCalculator = CubicCalculator(0.0, start)

    /** Send tokens per second. */
    var fillRate: Double = MIN_FILL_RATE
        private set
    var maxCapacity: Double = MIN_CAPACITY
        private set
    var capacity: Double = 0.0
        private set
    private var lastRefill: Double? = null

    /** Whether sends are rate limited; false until the first throttling response. */
    var enabled: Boolean = false
        private set

    private fun refill(now: Double) {
        lastRefill?.let { capacity = minOf(maxCapacity, capacity + (now - it) * fillRate) }
        lastRefill = now
    }

    /** Takes one send token, possibly on credit; returns how many seconds to wait before sending. */
    fun beforeSend(now: Double): Double {
        if (!enabled) return 0.0
        refill(now)
        capacity -= 1
        // Taking the token before sleeping gives concurrent callers first-come order.
        return if (capacity >= 0) 0.0 else -capacity / fillRate
    }

    /** Returns a token taken by [beforeSend] whose send was cancelled. */
    fun giveBack() {
        if (enabled) capacity = minOf(maxCapacity, capacity + 1)
    }

    /** Records an attempt that reached the transport; only a modeled throttling error is [throttled]. */
    fun afterAttempt(throttled: Boolean, now: Double) {
        val bucket = floor(now / TIME_BUCKET_RANGE) * TIME_BUCKET_RANGE
        count += 1
        if (bucket > lastBucket) {
            measuredRate = count / (bucket - lastBucket) * SMOOTHING + measuredRate * (1 - SMOOTHING)
            count = 0
            lastBucket = bucket
        }
        val newRate = if (throttled) {
            val rate = if (enabled) minOf(measuredRate, fillRate) else measuredRate
            enabled = true
            cubic.throttle(rate, now)
        } else cubic.success(now)
        val limited = minOf(newRate, MAX_RATE_ADJUST_SCALE * measuredRate)
        // Tokens earned at the old rate are credited before the rate changes.
        refill(now)
        fillRate = maxOf(limited, MIN_FILL_RATE)
        maxCapacity = maxOf(limited, MIN_CAPACITY)
        capacity = minOf(capacity, maxCapacity)
    }

    companion object {
        const val MIN_FILL_RATE: Double = 0.5
        const val MIN_CAPACITY: Double = 1.0
        const val SMOOTHING: Double = 0.8
        const val TIME_BUCKET_RANGE: Double = 0.5
        const val BETA: Double = 0.7
        const val SCALE_CONSTANT: Double = 0.4
        const val MAX_RATE_ADJUST_SCALE: Double = 2.0
    }
}
