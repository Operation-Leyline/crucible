import kotlin.math.pow
import kotlin.random.Random
import kotlin.time.Duration
import kotlin.time.Duration.Companion.seconds
import kotlin.time.DurationUnit
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/** One client's retry state: its mode, quota and rate limiter, shared by every call. */
internal class RetryStrategy(mode: RetryMode, private val clock: RuntimeClock, private val baseDelay: Duration, private val maxDelay: Duration) {
    val quota: RetryQuota? = if (mode == RetryMode.LEGACY) null else RetryQuota()
    private val limiter = if (mode == RetryMode.ADAPTIVE) AdaptiveRateLimiter(seconds()) else null
    private val lock = Mutex()

    private fun seconds(): Double = clock.now().toDouble(DurationUnit.SECONDS)

    /** How long to wait for a send token before an attempt (adaptive mode only). */
    suspend fun sendDelay(): Duration {
        val limiter = limiter ?: return Duration.ZERO
        return lock.withLock { limiter.beforeSend(seconds()) }.seconds
    }

    suspend fun cancelSend() {
        val limiter = limiter ?: return
        withContext(NonCancellable) { lock.withLock { limiter.giveBack() } }
    }

    suspend fun afterAttempt(throttled: Boolean) {
        val limiter = limiter ?: return
        withContext(NonCancellable) { lock.withLock { limiter.afterAttempt(throttled, seconds()) } }
    }

    /** Whether a retry may proceed, and the quota cost it consumed. */
    suspend fun acquireRetry(timeout: Boolean): Pair<Boolean, Int?> {
        val quota = quota ?: return true to null
        val cost = quota.acquire(timeout)
        return (cost != null) to cost
    }

    suspend fun succeeded(lastRetryCost: Int?) { quota?.release(lastRetryCost) }

    /** Full-jitter exponential backoff after the given (1-based) failed attempt. */
    fun backoff(attempt: Int): Duration =
        (Random.nextDouble() * minOf(maxDelay.toDouble(DurationUnit.SECONDS),
            baseDelay.toDouble(DurationUnit.SECONDS) * 2.0.pow(minOf(attempt - 1, 30)))).seconds
}
