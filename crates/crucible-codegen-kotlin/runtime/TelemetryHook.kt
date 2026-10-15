import kotlin.time.Duration

/**
 * Receives one call's events in order: [operationStarted], an [attemptStarted]/[attemptFinished]
 * pair per attempt that reached the transport, then [operationFinished]. Override any of them.
 *
 * Events never contain bodies, inputs, outputs or error messages; credential and `@sensitive`
 * header, query and label values are replaced with `***`. A hook that throws is logged and
 * otherwise ignored: telemetry never changes a call's result, retries or timing. Hooks run on
 * the calling coroutine and should return quickly.
 */
interface TelemetryHook {
    fun operationStarted(event: OperationStarted) {}
    fun attemptStarted(event: AttemptStarted) {}
    fun attemptFinished(event: AttemptFinished) {}
    fun operationFinished(event: OperationFinished) {}
}

/** A call began. [time] is on the client's [RuntimeClock]. */
data class OperationStarted(
    val invocationId: String,
    val service: String,
    val operation: String,
    val time: Duration,
)

/** An attempt is being sent. */
data class AttemptStarted(
    val invocationId: String,
    val service: String,
    val operation: String,
    /** 1-based. */
    val attempt: Int,
    val method: String,
    /** Scheme, host, port and base path; never user information. */
    val endpoint: String,
    /** Path and query as sent, with redactions. */
    val target: String,
    /** Request headers as sent (after middleware and authentication), lowercased, with redactions. */
    val headers: Map<String, String>,
    /** The `Content-Length` sent, or null for no body. */
    val requestBytes: Long?,
    val time: Duration,
)

/** An attempt ended. */
data class AttemptFinished(
    val invocationId: String,
    val service: String,
    val operation: String,
    val attempt: Int,
    /** The HTTP status, or null when no response arrived. */
    val status: Int?,
    /** A modeled or wire error's name, or the exception type name; never a message. */
    val error: String?,
    val requestId: String?,
    /** Bytes read. */
    val responseBytes: Long?,
    /** From the attempt being sent to its end, on the client's [RuntimeClock]. */
    val latency: Duration,
    /** Set only when another attempt follows: `throttling`, `transient`, `timeout`, `network` or `endpoint`. */
    val retryReason: String?,
    /** The sleep before the next attempt, when one follows. */
    val retryDelay: Duration?,
    val time: Duration,
)

/** A call ended. [status] is the last attempt's status. */
data class OperationFinished(
    val invocationId: String,
    val service: String,
    val operation: String,
    val attempts: Int,
    val status: Int?,
    val error: String?,
    val latency: Duration,
    val time: Duration,
)
