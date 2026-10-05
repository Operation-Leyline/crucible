import io.ktor.utils.io.ByteReadChannel
import io.ktor.utils.io.readAvailable
import io.ktor.utils.io.writeFully
import io.ktor.utils.io.writer
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * A streaming blob body that is never buffered by the client.
 *
 * A one-shot stream wraps a single channel: it can be opened once, so an operation
 * that sends it is never retried. A replayable stream calls its factory for every
 * attempt; the factory must reproduce the same bytes each time. [contentLength] is
 * checked against the bytes actually read, and [sha256] (lowercase or uppercase
 * hex) supplies the payload digest that signing and `x-amz-content-sha256` need
 * without reading the body twice.
 */
class ByteStream private constructor(
    val contentLength: Long?,
    val replayable: Boolean,
    val sha256: String?,
    private val source: suspend () -> ByteReadChannel,
    private val release: () -> Unit,
    private val limits: Limits?,
    private val parent: ByteStream? = null,
) : AutoCloseable {
    // Modeled bounds and response verification are enforced while bytes pass through.
    internal class Limits(val minimum: Long?, val maximum: Long?, val verifier: Checksums.Verifier?)
    private val guard = Mutex()
    private val closed = CompletableDeferred<Unit>()
    private var opened = false
    // A failure detected while piping; Ktor wraps channel causes, so the runtime rethrows the original.
    internal var failure: Throwable? = null
        private set
    init {
        require(contentLength == null || contentLength >= 0) { "Negative stream length" }
        require(sha256 == null || sha256.length == 64 && sha256.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }) { "SHA-256 must be 64 hexadecimal digits" }
        closed.invokeOnCompletion { release() }
    }

    // False once a one-shot body (or the body this one checks) was opened, or after close.
    internal val openable: Boolean get() = !closed.isCompleted && (replayable || !opened) && parent?.openable != false

    /** Opens the body. The caller reads or cancels the returned channel; a one-shot stream opens once. */
    suspend fun openChannel(): ByteReadChannel {
        val input = guard.withLock {
            check(!closed.isCompleted) { "Byte stream is closed" }
            check(replayable || !opened) { "One-shot byte stream has already been opened" }
            opened = true
            source()
        }
        val limits = limits ?: return input
        val job = CoroutineScope(currentCoroutineContext()).writer(autoFlush = true) {
            val buffer = ByteArray(16 * 1024); var count = 0L
            // Only violations detected here are recorded; transport and cancellation errors pass through.
            fun fail(error: Throwable): Nothing { failure = error; throw error }
            try {
                while (true) {
                    val size = input.readAvailable(buffer, 0, buffer.size)
                    if (size < 0) break
                    count += size
                    if (contentLength != null && count > contentLength || limits.maximum != null && count > limits.maximum) fail(IllegalArgumentException("Stream exceeds its declared length"))
                    limits.verifier?.update(buffer, 0, size)
                    channel.writeFully(buffer, 0, size)
                }
                if (contentLength != null && count != contentLength || limits.minimum != null && count < limits.minimum) fail(IllegalArgumentException("Stream ended before its declared length"))
                try { limits.verifier?.finish() } catch (error: ChecksumMismatch) { fail(error) }
            } catch (error: CancellationException) {
                // A cancelled copy must not look like a complete body to its reader.
                channel.cancel(error); throw error
            } finally { input.cancel(null) }
        }
        val registration = closed.invokeOnCompletion { input.cancel(null); job.channel.cancel(null) }
        job.job.invokeOnCompletion { registration.dispose() }
        return job.channel
    }

    /** Reads the whole body into memory, failing when it exceeds [limit] bytes. */
    suspend fun toByteArray(limit: Int): ByteArray {
        require(limit >= 0) { "Negative limit" }
        val channel = openChannel()
        try {
            val buffer = ByteArray(8192); var bytes = ByteArray(0); var size = 0
            while (true) {
                val read = channel.readAvailable(buffer, 0, buffer.size)
                if (read < 0) break
                check(read <= limit - size) { "Stream exceeds $limit bytes" }
                if (size + read > bytes.size) bytes = bytes.copyOf(maxOf(size + read, minOf(limit.toLong(), bytes.size * 2L).toInt()))
                buffer.copyInto(bytes, size, 0, read); size += read
            }
            return if (size == bytes.size) bytes else bytes.copyOf(size)
        } finally { channel.cancel(null) }
    }

    /** Releases the source. An unread one-shot body is cancelled, and open channels are cancelled. */
    override fun close() { closed.complete(Unit) }

    internal fun checked(minimum: Long?, maximum: Long?, requiresLength: Boolean, verifier: Checksums.Verifier? = null): ByteStream {
        require(!requiresLength || contentLength != null) { "This operation requires a known stream length" }
        require(contentLength == null || (minimum == null || contentLength >= minimum) && (maximum == null || contentLength <= maximum)) { "Stream length violates modeled bounds" }
        return ByteStream(contentLength, replayable, sha256, { openChannel() }, {}, Limits(minimum, maximum, verifier), this)
    }

    companion object {
        /** A replayable stream; [factory] must return a fresh channel with the same bytes on every call. */
        fun replay(contentLength: Long? = null, sha256: String? = null, factory: suspend () -> ByteReadChannel): ByteStream =
            ByteStream(contentLength, true, sha256, factory, {}, null)
        /** A one-shot stream over [channel]; closing the stream cancels an unread channel. */
        fun once(channel: ByteReadChannel, contentLength: Long? = null, sha256: String? = null): ByteStream =
            ByteStream(contentLength, false, sha256, { channel }, { channel.cancel(null) }, null)
        /** A replayable stream over a snapshot of [bytes], with its length and SHA-256. */
        fun fromBytes(bytes: ByteArray): ByteStream {
            val snapshot = bytes.copyOf()
            return replay(snapshot.size.toLong(), sha256(snapshot)) { ByteReadChannel(snapshot) }
        }
    }
}
