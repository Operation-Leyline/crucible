/**
 * An error response from the service. Modeled errors are generated subclasses of the service
 * exception, which extends this class; the details of the response are available here.
 */
open class ServiceError : RuntimeException() {
    private var detail = "Service request failed"

    /** The error message from the response. */
    override val message: String get() = detail

    /** The error code as it appeared on the wire. */
    var errorName: String = "UnknownError"
        private set

    /** The HTTP status code of the response. */
    var status: Int = 0
        private set

    /** The request id the service reported, if any. */
    var requestId: String? = null
        private set

    /** The response headers. */
    var headers: Map<String, List<String>> = emptyMap()
        private set

    /** Whether the client treats the error as retryable. */
    var retryable: Boolean = false
        private set

    /** Whether the client treats the error as throttling. */
    var throttling: Boolean = false
        private set
    // BEGIN ERROR DOCUMENTATION
    /** Static documentation for the modeled error code and its known reason. */
    var docUrl: String? = null
        private set
    internal fun initializeDocumentation(url: String?) { docUrl = url }
    // END ERROR DOCUMENTATION
    internal fun initialize(name: String, message: String, status: Int, requestId: String?, headers: Map<String, List<String>>, retryable: Boolean, throttling: Boolean) {
        errorName = name; detail = message; this.status = status; this.requestId = requestId
        this.headers = headers.mapValues { it.value.toList() }.toMap()
        this.retryable = retryable; this.throttling = throttling
    }
}

/** The HTTP transport failed before a response arrived (connection, TLS or timeout); see the cause. */
class NetworkError(cause: Throwable) : RuntimeException(cause.message, cause)
