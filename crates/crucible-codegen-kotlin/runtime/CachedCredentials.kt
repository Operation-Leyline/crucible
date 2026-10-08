import kotlin.time.Clock
import kotlin.time.Duration
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * Caches the result of a credential provider and refreshes it shortly before it expires. One
 * refresh is in flight at a time; cancellation releases the lock and failed refreshes are not cached.
 *
 * @property provider the underlying source
 * @property refreshBefore how long before expiry to fetch again; not negative
 * @property clock the time source
 */
class CachedCredentials(private val provider: suspend () -> AwsCredentials?,
    private val refreshBefore: Duration = 1.minutes, private val clock: () -> Instant = { Clock.System.now() }) {
    private val mutex = Mutex()
    private var value: AwsCredentials? = null
    init { require(!refreshBefore.isNegative() && refreshBefore.isFinite()) }
    /** Returns cached credentials, refreshing them first when close to expiry. */
    suspend fun get(): AwsCredentials? = mutex.withLock {
        val now = clock()
        value?.let { if (it.expiresAt == null || it.expiresAt > now + refreshBefore) return@withLock it }
        val next = provider()
        require(next?.expiresAt == null || next.expiresAt > clock()) { "Credential provider returned expired credentials" }
        value = next; next
    }
}

/**
 * Tries providers in order and stops at the first identity.
 *
 * @param providers the sources, in priority order
 * @return a provider yielding null when none has credentials
 */
fun credentialChain(vararg providers: suspend () -> AwsCredentials?): suspend () -> AwsCredentials? {
    val snapshot = providers.copyOf()
    return suspend {
        var result: AwsCredentials? = null
        for (provider in snapshot) { result = provider(); if (result != null) break }
        result
    }
}
