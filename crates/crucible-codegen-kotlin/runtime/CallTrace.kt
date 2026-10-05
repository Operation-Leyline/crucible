import io.ktor.http.decodeURLQueryComponent
import io.ktor.util.logging.KtorSimpleLogger
import io.ktor.util.logging.Logger
import kotlin.time.Duration
import kotlin.uuid.Uuid

private val telemetryLog: Logger by lazy { KtorSimpleLogger("crucible.telemetry") }

/** One call's telemetry. Without a hook it only counts attempts. */
internal class CallTrace(private val hook: TelemetryHook?, private val clock: RuntimeClock, private val service: String, private val operation: String) {
    var attempts: Int = 0
        private set
    private var status: Int? = null
    private val id = if (hook != null) Uuid.random().toHexString() else ""
    private val start = if (hook != null) clock.now() else Duration.ZERO
    private var attemptStart = Duration.ZERO

    init { emit("operationStarted") { it.operationStarted(OperationStarted(id, service, operation, start)) } }

    private inline fun emit(name: String, block: (TelemetryHook) -> Unit) {
        val hook = hook ?: return
        try { block(hook) } catch (error: Exception) { telemetryLog.error("telemetry hook $name failed", error) }
    }

    fun attemptStarted(method: String, endpoint: String, source: String, target: String, headers: Map<String, String>, requestBytes: Long?) {
        attempts++
        if (hook == null) return
        attemptStart = clock.now()
        emit("attemptStarted") { it.attemptStarted(AttemptStarted(id, service, operation, attempts, method, endpoint, source, target, headers, requestBytes, attemptStart)) }
    }

    fun attemptFinished(status: Int?, error: Throwable?, requestId: String?, responseBytes: Long?, retryReason: String? = null, retryDelay: Duration? = null) {
        this.status = status
        if (hook == null) return
        val now = clock.now()
        emit("attemptFinished") {
            it.attemptFinished(AttemptFinished(id, service, operation, attempts, status, errorName(error), requestId, responseBytes, now - attemptStart, retryReason, retryDelay, now))
        }
    }

    fun finished(error: Throwable?) {
        if (hook == null) return
        val now = clock.now()
        emit("operationFinished") { it.operationFinished(OperationFinished(id, service, operation, attempts, status, errorName(error), now - start, now)) }
    }

    companion object {
        const val REDACTED: String = "***"
        /** Always redacted, whatever the model says. */
        val CREDENTIAL_HEADERS: Set<String> = setOf("authorization", "proxy-authorization", "cookie", "set-cookie", "x-amz-security-token")
        val CREDENTIAL_QUERY: Set<String> = setOf("x-amz-signature", "x-amz-credential", "x-amz-security-token")

        /** A modeled or wire error's name, or the exception type name; never a message, which may echo request data. */
        fun errorName(error: Throwable?): String? = when (error) {
            null -> null
            is ServiceError -> error.errorName
            else -> error::class.simpleName ?: "Throwable"
        }

        fun redactHeaders(headers: Map<String, String>, sensitive: Set<String>): Map<String, String> =
            headers.entries.associate { (name, value) ->
                val lower = name.lowercase()
                lower to if (lower in CREDENTIAL_HEADERS || lower in sensitive) REDACTED else value
            }

        /** Names compare case-insensitively: redacting too much is the safe failure. */
        fun redactQuery(encodedQuery: String, sensitive: Set<String>): String =
            encodedQuery.split('&').filter { it.isNotEmpty() }.joinToString("&") { pair ->
                val name = pair.substringBefore('=')
                val decoded = try { name.decodeURLQueryComponent() } catch (_: Exception) { name }.lowercase()
                if (decoded in sensitive || decoded in CREDENTIAL_QUERY) "$name=$REDACTED" else pair
            }

        /** Scheme, host, port and base path of an endpoint; never user information. */
        fun displayEndpoint(endpoint: String): String {
            val scheme = endpoint.substringBefore("://", "")
            val rest = endpoint.substringAfter("://")
            val authority = rest.substringBefore('/')
            return (if (scheme.isEmpty()) "" else "$scheme://") + authority.substringAfterLast('@') + rest.drop(authority.length)
        }
    }
}
