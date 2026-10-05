import io.ktor.client.HttpClient
import io.ktor.http.Url
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

/** Configuration is copied by the client. An injected Ktor HttpClient remains caller-owned. */
data class ClientConfig(
    val endpoint: String? = null,
    val httpClient: HttpClient? = null,
    val token: (suspend () -> String?)? = null,
    val basicAuth: (suspend () -> BasicCredentials?)? = null,
    val apiKey: (suspend () -> String?)? = null,
    val credentials: (suspend () -> AwsCredentials?)? = null,
    val region: String? = null,
    val endpointParameters: Map<String, kotlinx.serialization.json.JsonElement> = emptyMap(),
    val endpointBuiltins: Map<String, kotlinx.serialization.json.JsonElement> = emptyMap(),
    val disableHostPrefix: Boolean = false,
    val clock: () -> kotlin.time.Instant = { kotlin.time.Clock.System.now() },
    val requestChecksumCalculation: ChecksumPolicy = ChecksumPolicy.WHEN_SUPPORTED,
    val responseChecksumValidation: ChecksumPolicy = ChecksumPolicy.WHEN_SUPPORTED,
    val checksumValidated: ((operation: String, algorithm: String) -> Unit)? = null,
    val maxAttempts: Int = 3,
    val timeout: Duration = 30.seconds,
    val baseDelay: Duration = 100.milliseconds,
    val maxDelay: Duration = 20.seconds,
    val headers: Map<String, String> = emptyMap(),
    val middleware: List<suspend (Request) -> Unit> = emptyList(),
    /** Retry limits on top of retry eligibility; see [RetryMode]. */
    val retryMode: RetryMode = RetryMode.LEGACY,
    /** Endpoint discovery: null discovers for required operations only, true also for optional ones, false never (required operations fail). */
    val endpointDiscovery: Boolean? = null,
    /** Receives redacted call and attempt events. */
    val telemetry: TelemetryHook? = null,
    /** Monotonic time and sleeps for backoff, rate limiting, discovery expiry and telemetry; a test seam. */
    val runtimeClock: RuntimeClock = RuntimeClock.System,
)

/** Middleware can replace the body; the optional digest hashes bytes after every hook. */
class Request(var uri: Url, val method: String, val headers: MutableMap<String, String>, body: ByteArray?, stream: ByteStream? = null) {
    var body: ByteArray? = body
        set(value) { field=value;if(value!=null)this.stream=null }
    var stream: ByteStream? = stream
        set(value) { field=value;if(value!=null)this.body=null }
}
