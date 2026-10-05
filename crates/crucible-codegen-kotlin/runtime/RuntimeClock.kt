import kotlin.time.Duration
import kotlin.time.TimeSource
import kotlinx.coroutines.delay

/**
 * Monotonic time and sleeps for retry backoff, adaptive rate limiting, endpoint discovery cache
 * expiry, and telemetry timestamps and latencies. Tests inject a fake clock.
 *
 * A call's own `timeout` always uses the platform clock, so a fake clock never shortens or
 * extends a real deadline. (`ClientConfig.clock` is a different clock: the wall clock used to sign.)
 */
interface RuntimeClock {
    /** Time elapsed since an arbitrary fixed origin; never decreases. */
    fun now(): Duration

    /** Suspends for [duration]; must cooperate with coroutine cancellation. */
    suspend fun sleep(duration: Duration)

    companion object {
        /** The platform monotonic clock and coroutine `delay`. */
        val System: RuntimeClock = object : RuntimeClock {
            private val origin = TimeSource.Monotonic.markNow()
            override fun now(): Duration = origin.elapsedNow()
            override suspend fun sleep(duration: Duration) = delay(duration)
        }
    }
}
