open class ServiceError : RuntimeException() {
    private var detail = "Service request failed"
    override val message: String get() = detail
    var errorName: String = "UnknownError"
        private set
    var status: Int = 0
        private set
    var requestId: String? = null
        private set
    var headers: Map<String, List<String>> = emptyMap()
        private set
    var retryable: Boolean = false
        private set
    var throttling: Boolean = false
        private set
    internal fun initialize(name: String, message: String, status: Int, requestId: String?, headers: Map<String, List<String>>, retryable: Boolean, throttling: Boolean) {
        errorName = name; detail = message; this.status = status; this.requestId = requestId
        this.headers = headers.mapValues { it.value.toList() }.toMap()
        this.retryable = retryable; this.throttling = throttling
    }
}

class NetworkError(cause: Throwable) : RuntimeException(cause.message, cause)
