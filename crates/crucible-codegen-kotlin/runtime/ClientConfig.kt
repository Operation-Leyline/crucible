import io.ktor.client.HttpClient
import io.ktor.http.Url
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

/**
 * Settings for a generated client, copied when the client is constructed: changing a map or list
 * you passed in later does not affect the client. An injected Ktor `HttpClient` remains
 * caller-owned and is never closed by the SDK.
 *
 * @property endpoint base URL, absolute HTTP(S) with no credentials, query or fragment; defaults to
 *   the endpoint the SDK was generated with, and is required when it was generated with none
 * @property httpClient transport used for every call; the client builds and later closes a default
 *   engine when this is null
 * @property token supplies the bearer token for HTTP bearer auth, called once per attempt
 * @property maxAttempts total attempts per call including the first; at least 1
 * @property timeout deadline for a whole call including retries
 * @property baseDelay base of the exponential, fully jittered retry backoff
 * @property maxDelay upper bound of a single retry delay
 * @property headers headers added to every request before middleware runs
 * @property middleware hooks run in order on each attempt's [Request] before it is sent
 * @property retryMode retry limits on top of retry eligibility; see [RetryMode]
 * @property telemetry receives redacted call and attempt events
 * @property runtimeClock monotonic time and sleeps for backoff, rate limiting and telemetry; a test seam
 */
data class ClientConfig(
    val endpoint: String? = null,
    val httpClient: HttpClient? = null,
    val token: (suspend () -> String?)? = null,
    val maxAttempts: Int = 3,
    val timeout: Duration = 30.seconds,
    val baseDelay: Duration = 100.milliseconds,
    val maxDelay: Duration = 20.seconds,
    val headers: Map<String, String> = emptyMap(),
    val middleware: List<suspend (Request) -> Unit> = emptyList(),
    val retryMode: RetryMode = RetryMode.LEGACY,
    val telemetry: TelemetryHook? = null,
    val runtimeClock: RuntimeClock = RuntimeClock.System,
)

/**
 * An outgoing HTTP request, handed to each [ClientConfig.middleware] hook. A hook may replace the
 * body or the URL.
 *
 * @property uri target URL; a hook may replace it
 * @property method HTTP method
 * @property headers mutable request headers
 * @property body body bytes, or null for no body
 */
class Request(var uri: Url, val method: String, val headers: MutableMap<String, String>, var body: ByteArray?)
