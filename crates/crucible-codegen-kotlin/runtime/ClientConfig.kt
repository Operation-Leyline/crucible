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
 *   the endpoint the SDK was generated with, and is required when the model has neither that nor
 *   endpoint rules
 * @property httpClient transport used for every call; the client builds and later closes a default
 *   engine when this is null
 * @property token supplies the bearer token for HTTP bearer auth, called once per attempt
 * @property basicAuth supplies HTTP Basic credentials, called once per attempt
 * @property apiKey supplies the API key for API key auth, called once per attempt
 * @property credentials supplies AWS credentials for SigV4 signing, called once per attempt so a
 *   provider such as [CachedCredentials] can refresh them
 * @property region AWS region used for SigV4 signing and endpoint rules
 * @property endpointParameters values for the endpoint rule set's parameters, by name
 * @property endpointBuiltins overrides for endpoint rule built-ins such as `AWS::UseFIPS`
 * @property disableHostPrefix suppresses the modeled `@endpoint` host prefix
 * @property clock wall clock used for signing; replace it in tests
 * @property requestChecksumCalculation when request checksums are computed
 * @property responseChecksumValidation when response checksums are validated
 * @property checksumValidated called with the operation and algorithm after a response checksum validated
 * @property maxAttempts total attempts per call including the first; at least 1
 * @property timeout deadline for a whole call including retries
 * @property baseDelay base of the exponential, fully jittered retry backoff
 * @property maxDelay upper bound of a single retry delay
 * @property headers headers added to every request before middleware runs
 * @property middleware hooks run in order on each attempt's [Request] before it is sent
 * @property retryMode retry limits on top of retry eligibility; see [RetryMode]
 * @property endpointDiscovery null discovers for required operations only, true also for optional ones, false never (required operations fail)
 * @property telemetry receives redacted call and attempt events
 * @property runtimeClock monotonic time and sleeps for backoff, rate limiting, discovery expiry and telemetry; a test seam
 */
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
    val retryMode: RetryMode = RetryMode.LEGACY,
    val endpointDiscovery: Boolean? = null,
    val telemetry: TelemetryHook? = null,
    val runtimeClock: RuntimeClock = RuntimeClock.System,
)

/**
 * An outgoing HTTP request, handed to each [ClientConfig.middleware] hook. A hook may replace the
 * body; the optional content digest hashes the bytes after every hook ran. Setting [body] clears
 * [stream] and vice versa.
 *
 * @property uri target URL; a hook may replace it
 * @property method HTTP method
 * @property headers mutable request headers
 */
class Request(var uri: Url, val method: String, val headers: MutableMap<String, String>, body: ByteArray?, stream: ByteStream? = null) {
    /** Body bytes, or null for no body or a streaming payload. */
    var body: ByteArray? = body
        set(value) { field=value;if(value!=null)this.stream=null }
    /** The streaming payload, or null. */
    var stream: ByteStream? = stream
        set(value) { field=value;if(value!=null)this.body=null }
}
