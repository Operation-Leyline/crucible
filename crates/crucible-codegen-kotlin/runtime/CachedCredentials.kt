import kotlin.time.Clock
import kotlin.time.Duration
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/** One refresh in flight. Cancellation releases the lock; failed refreshes are not cached. */
class CachedCredentials(private val provider: suspend () -> AwsCredentials?,
    private val refreshBefore: Duration = 1.minutes, private val clock: () -> Instant = { Clock.System.now() }) {
    private val mutex = Mutex()
    private var value: AwsCredentials? = null
    init { require(!refreshBefore.isNegative() && refreshBefore.isFinite()) }
    suspend fun get(): AwsCredentials? = mutex.withLock {
        val now = clock()
        value?.let { if (it.expiresAt == null || it.expiresAt > now + refreshBefore) return@withLock it }
        val next = provider()
        require(next?.expiresAt == null || next.expiresAt > clock()) { "Credential provider returned expired credentials" }
        value = next; next
    }
}

/** Tries providers in order and stops at the first identity. */
fun credentialChain(vararg providers: suspend () -> AwsCredentials?): suspend () -> AwsCredentials? {
    val snapshot = providers.copyOf()
    return suspend {
        var result: AwsCredentials? = null
        for (provider in snapshot) { result = provider(); if (result != null) break }
        result
    }
}
