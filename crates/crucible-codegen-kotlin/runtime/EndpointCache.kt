import kotlin.coroutines.ContinuationInterceptor
import kotlin.coroutines.EmptyCoroutineContext
import kotlin.time.Duration
import kotlin.time.Duration.Companion.seconds
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/** A discovered endpoint URL and the clock reading at which it expires. Compared by identity. */
internal class DiscoveredAddress(val url: String, val expires: Duration)

/**
 * One client's discovered endpoints, keyed by what is sent to the discovery operation.
 * Concurrent calls needing the same key share one discovery, which runs in the client's own
 * scope: cancelling the call that started it does not cancel it for the others.
 */
internal class EndpointCache(private val clock: RuntimeClock, private val scope: CoroutineScope) {
    private val lock = Mutex()
    private val entries = mutableMapOf<String, DiscoveredAddress>()
    private val flights = mutableMapOf<String, CompletableDeferred<DiscoveredAddress>>()
    private val failures = mutableMapOf<String, Duration>()

    /** Whether an optional operation should keep using the configured endpoint for this key. */
    suspend fun failedRecently(key: String): Boolean = lock.withLock {
        val expires = failures[key] ?: return@withLock false
        if (clock.now() >= expires) { failures.remove(key); false } else true
    }

    suspend fun recordFailure(key: String) {
        withContext(NonCancellable) { lock.withLock { failures[key] = clock.now() + FAILURE_TTL } }
    }

    /** Evicts an entry a response rejected, unless a newer one already replaced it. */
    suspend fun invalidate(key: String, entry: DiscoveredAddress) {
        withContext(NonCancellable) { lock.withLock { if (entries[key] === entry) entries.remove(key) } }
    }

    /** A valid cached entry, or the result of the one discovery shared by every caller of this key. */
    suspend fun resolve(key: String, fetch: suspend () -> DiscoveredAddress): DiscoveredAddress {
        val dispatcher = currentCoroutineContext()[ContinuationInterceptor]
        val flight = lock.withLock {
            entries[key]?.takeIf { clock.now() < it.expires }?.let { return it }
            flights[key]?.takeUnless { it.isCompleted } ?: CompletableDeferred<DiscoveredAddress>().also { started ->
                flights[key] = started
                val job = scope.launch(dispatcher ?: EmptyCoroutineContext) {
                    val entry = try { fetch() } catch (error: Throwable) {
                        withContext(NonCancellable) { lock.withLock { if (flights[key] === started) flights.remove(key) } }
                        started.completeExceptionally(error)
                        return@launch
                    }
                    withContext(NonCancellable) {
                        lock.withLock {
                            if (flights[key] === started) flights.remove(key)
                            // A zero period serves only the callers that waited for this discovery.
                            if (clock.now() < entry.expires) entries[key] = entry else entries.remove(key)
                        }
                    }
                    started.complete(entry)
                }
                // A client closed before the discovery started still releases its waiters.
                job.invokeOnCompletion { cause -> if (cause != null) started.completeExceptionally(cause) }
            }
        }
        return flight.await()
    }

    companion object {
        /** How long an optional operation keeps using the configured endpoint after discovery failed. */
        val FAILURE_TTL: Duration = 60.seconds
    }
}
