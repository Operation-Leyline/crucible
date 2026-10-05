import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/**
 * A token bucket bounding how much of one client's traffic may be retries ([RetryMode.STANDARD]
 * and [RetryMode.ADAPTIVE]). One quota is shared by every call of a client.
 */
class RetryQuota(available: Int = INITIAL_RETRY_TOKENS, val capacity: Int = INITIAL_RETRY_TOKENS) {
    private val lock = Mutex()

    /** The tokens currently available. */
    @kotlin.concurrent.Volatile
    var available: Int = available
        private set

    /** The cost of one retry, or null (spending nothing) when the quota cannot pay for it. */
    suspend fun acquire(timeout: Boolean): Int? = lock.withLock {
        val cost = if (timeout) RETRY_TIMEOUT_COST else RETRY_COST
        if (cost > available) null else { available -= cost; cost }
    }

    /** A successful call refunds its last retry's cost, or adds a small increment if it never retried. */
    suspend fun release(lastRetryCost: Int?) {
        // A refund is bookkeeping for a call that already succeeded; cancellation must not lose it.
        withContext(NonCancellable) {
            lock.withLock { available = minOf(capacity, available + (lastRetryCost ?: NO_RETRY_INCREMENT)) }
        }
    }

    companion object {
        const val INITIAL_RETRY_TOKENS: Int = 500
        const val RETRY_COST: Int = 5
        const val RETRY_TIMEOUT_COST: Int = 10
        const val NO_RETRY_INCREMENT: Int = 1
    }
}
