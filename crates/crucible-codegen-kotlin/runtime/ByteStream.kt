import io.ktor.utils.io.ByteReadChannel
import io.ktor.utils.io.readAvailable
import io.ktor.utils.io.writeFully
import io.ktor.utils.io.writer
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/** A bounded byte stream. A one-shot channel can be opened once; replay factories open anew per attempt. */
class ByteStream private constructor(
    val contentLength: Long?,
    val replayable: Boolean,
    val sha256: String?,
    private val factory: suspend () -> ByteReadChannel,
    private val release: () -> Unit = {},
    private val minimum: Long? = null,
    private val maximum: Long? = null,
) : AutoCloseable {
    private val guard=Mutex()
    private val closed=CompletableDeferred<Unit>()
    private var opened=false
    init {
        require(contentLength==null || contentLength>=0) { "Negative stream length" }
        require(sha256==null || Regex("[0-9a-fA-F]{64}").matches(sha256)) { "SHA-256 must be 64 hexadecimal digits" }
        closed.invokeOnCompletion { release() }
    }
    /** The caller owns the returned channel. Cancelling it closes the source. */
    suspend fun openChannel(): ByteReadChannel {
        val source=guard.withLock {
            check(!closed.isCompleted) { "Byte stream is closed" }
            check(replayable || !opened) { "One-shot byte stream has already been consumed" }
            opened=true
            factory()
        }
        val writer=CoroutineScope(currentCoroutineContext()).writer(autoFlush=true) {
            val bytes=ByteArray(16*1024);var count=0L
            try {
                while(true) {
                    currentCoroutineContext().ensureActive()
                    val size=source.readAvailable(bytes)
                    if(size<0)break
                    if(size==0)continue
                    check(count<=Long.MAX_VALUE-size) { "Stream length overflow" };count+=size
                    check(contentLength==null || count<=contentLength) { "Stream exceeds declared length" }
                    check(maximum==null || count<=maximum) { "Stream exceeds modeled maximum length" }
                    channel.writeFully(bytes,0,size)
                }
                check(contentLength==null || count==contentLength) { "Stream does not match declared length" }
                check(minimum==null || count>=minimum) { "Stream is below modeled minimum length" }
            } finally { source.cancel(null) }
        }
        val registration=closed.invokeOnCompletion { source.cancel(null);writer.channel.cancel(null) }
        writer.invokeOnCompletion { registration.dispose() }
        return writer.channel
    }
    override fun close() { closed.complete(Unit) }
    internal fun checked(min: Long?,max: Long?,requiresLength: Boolean): ByteStream {
        require(!requiresLength || contentLength!=null) { "This operation requires a known stream length" }
        require(contentLength==null || (min==null || contentLength>=min)&&(max==null || contentLength<=max)) { "Stream length violates modeled bounds" }
        return ByteStream(contentLength,replayable,sha256,{openChannel()},{close()},min,max)
    }
    companion object {
        /** The factory must return a fresh channel on every call. */
        fun replay(contentLength: Long? = null,sha256: String? = null,factory: suspend () -> ByteReadChannel): ByteStream = ByteStream(contentLength,true,sha256,factory)
        fun once(channel: ByteReadChannel,contentLength: Long? = null,sha256: String? = null): ByteStream = ByteStream(contentLength,false,sha256,{channel},{channel.cancel(null)})
        fun fromBytes(bytes: ByteArray): ByteStream { val snapshot=bytes.copyOf();return replay(snapshot.size.toLong()) {ByteReadChannel(snapshot)} }
    }
}
