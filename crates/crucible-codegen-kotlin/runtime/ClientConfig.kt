import java.net.URI
import java.time.Duration
import okhttp3.OkHttpClient

/** Configuration is copied by Client. An injected OkHttpClient remains caller-owned. */
data class ClientConfig(
    val endpoint: String? = null,
    val httpClient: OkHttpClient? = null,
    val token: (suspend () -> String?)? = null,
    val maxAttempts: Int = 3,
    val timeout: Duration = Duration.ofSeconds(30),
    val baseDelay: Duration = Duration.ofMillis(100),
    val maxDelay: Duration = Duration.ofSeconds(20),
    val headers: Map<String, String> = emptyMap(),
    val middleware: List<suspend (Request) -> Unit> = emptyList(),
)

/** Middleware can replace the body; the optional digest hashes bytes after every hook. */
class Request(var uri: URI, val method: String, val headers: MutableMap<String, String>, var body: ByteArray?)
