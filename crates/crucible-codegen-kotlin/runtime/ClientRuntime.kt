import com.google.gson.JsonElement
import com.google.gson.JsonObject
import java.io.Closeable
import java.io.IOException
import java.math.BigDecimal
import java.net.URI
import java.net.URLEncoder
import java.security.MessageDigest
import java.time.Duration
import java.time.Instant
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale
import java.util.Base64
import java.util.TreeMap
import java.util.UUID
import java.util.concurrent.TimeUnit
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlin.random.Random
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeout
import okhttp3.Call
import okhttp3.Callback
import okhttp3.OkHttpClient
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response

internal class ClientRuntime(input: ClientConfig, endpoint: String, private val digest: Boolean,
    private val schema: JsonObject = loadSchema()) : Closeable {
    private val config = input.copy(endpoint = (input.endpoint ?: endpoint).trimEnd('/'),
        headers = input.headers.toMap(), middleware = input.middleware.toList())
    private val ownsHttp = input.httpClient == null
    private val http = input.httpClient ?: OkHttpClient.Builder().followRedirects(false).followSslRedirects(false)
        // The coroutine deadline owns cancellation; OkHttp's shorter defaults must not preempt it.
        .connectTimeout(0, TimeUnit.MILLISECONDS).readTimeout(0, TimeUnit.MILLISECONDS).writeTimeout(0, TimeUnit.MILLISECONDS)
        .retryOnConnectionFailure(false).build()
    private val codec = JsonCodec(schema)
    init {
        val uri = URI(config.endpoint)
        require(uri.scheme in setOf("http", "https") && uri.host != null && uri.userInfo == null && uri.query == null && uri.fragment == null) {
            "Absolute HTTP(S) endpoint without credentials, query, or fragment required"
        }
        require(config.maxAttempts >= 1 && !config.timeout.isNegative && !config.timeout.isZero && config.timeout.toMillis() > 0 &&
            !config.baseDelay.isNegative && !config.maxDelay.isNegative) { "Invalid attempts, timeout, or retry delays" }
        config.baseDelay.toMillis(); config.maxDelay.toMillis()
    }
    override fun close() {
        if (ownsHttp) {
            http.dispatcher.executorService.shutdown()
            http.connectionPool.evictAll()
            http.cache?.close()
        }
    }
    private data class Prepared(val operation: JsonObject, val uri: URI, val headers: Map<String, String>, val body: ByteArray?)
    private data class WireResponse(val status: Int, val headers: Map<String, List<String>>, val body: ByteArray) {
        fun header(name: String): String? = headers[name.lowercase(Locale.ROOT)]?.joinToString(", ")
    }
    private fun operation(name: String): JsonObject = schema.getAsJsonObject("operations").getAsJsonObject(name)
    private fun prepare(name: String, values: JsonObject): Prepared {
        val op = operation(name); val binding = op.getAsJsonObject("request")
        op.text("token")?.let { if (!values.has(it) || values.get(it).isJsonNull) values.addProperty(it, UUID.randomUUID().toString()) }
        var uri = op.text("uri")!!
        binding.getAsJsonArray("labels").forEach {
            val key = it.asString; val value = values.get(key)
            require(value != null && !value.isJsonNull && codec.bound(value).isNotEmpty()) { "$key is required" }
            uri = uri.replace("{$key}", escape(codec.bound(value)))
        }
        val query = mutableListOf<String>()
        binding.getAsJsonArray("query").forEach {
            val pair = it.asJsonArray; val value = values.get(pair[0].asString)
            if (value != null && !value.isJsonNull) {
                val items = if (value.isJsonArray) value.asJsonArray.toList() else listOf(value)
                items.forEach { item -> query.add(escape(pair[1].asString) + "=" + escape(item.asString)) }
            }
        }
        if (query.isNotEmpty()) uri += (if ('?' in uri) "&" else "?") + query.joinToString("&")
        val headers = TreeMap<String, String>(String.CASE_INSENSITIVE_ORDER).also { it.putAll(config.headers) }
        binding.getAsJsonArray("headers").forEach {
            val pair = it.asJsonArray; val value = values.get(pair[0].asString)
            if (value != null && !value.isJsonNull) headers[pair[1].asString] = codec.bound(value)
        }
        var bytes: ByteArray? = null
        if (binding.getAsJsonArray("body").size() != 0) {
            val body = JsonObject()
            binding.getAsJsonArray("body").forEach { if (values.has(it.asString)) body.add(it.asString, values.get(it.asString).deepCopy()) }
            bytes = codec.stringify(body).toByteArray(Charsets.UTF_8); headers["Content-Type"] = "application/json"
        }
        binding.get("payload")?.takeIf { it.isJsonArray }?.asJsonArray?.let { payload ->
            values.get(payload[0].asString)?.takeUnless { it.isJsonNull }?.let { value ->
                val raw = Base64.getDecoder().decode(value.asString)
                if (raw.isNotEmpty()) { bytes = raw; headers["Content-Type"] = payload[1].asString }
            }
        }
        return Prepared(op, URI(config.endpoint + uri), headers, bytes)
    }
    suspend fun call(name: String, input: Any?): Any {
        currentCoroutineContext().ensureActive()
        val values = if (input == null) JsonObject() else codec.encode(operation(name).text("input"), input).asJsonObject
        return send(prepare(name, values))
    }
    private suspend fun send(prepared: Prepared): Any = withTimeout(config.timeout.toMillis()) {
        val op = prepared.operation
        var attempt = 0
        while (true) {
            currentCoroutineContext().ensureActive()
            attempt++
            val headers = TreeMap<String, String>(String.CASE_INSENSITIVE_ORDER).also { it.putAll(prepared.headers) }
            if (schema.flag("bearer")) config.token?.invoke()?.let { headers["Authorization"] = "Bearer $it" }
            val request = Request(prepared.uri, op.text("method")!!, headers, prepared.body?.clone())
            config.middleware.forEach { it(request) }
            require(request.uri.scheme in setOf("http", "https") && request.uri.host != null && request.uri.userInfo == null) { "Invalid middleware URI" }
            require(!request.headers.containsKey("Authorization") || secure(request.uri)) { "Credentials require HTTPS or loopback HTTP" }
            val body = request.body?.clone()
            if (digest && body != null && body.isNotEmpty()) headers["x-amz-content-sha256"] =
                MessageDigest.getInstance("SHA-256").digest(body).joinToString("") { "%02x".format(it.toInt() and 255) }
            val builder = okhttp3.Request.Builder().url(request.uri.toString())
            request.headers.forEach { (name, value) -> builder.header(name, value) }
            val requiresBody = request.method in setOf("POST", "PUT", "PATCH", "PROPPATCH", "REPORT")
            builder.method(request.method, (body ?: if (requiresBody) byteArrayOf() else null)?.toRequestBody())
            currentCoroutineContext().ensureActive()
            val response = try {
                val bindings = op.getAsJsonObject("response")
                exchange(builder.build(), bindings.getAsJsonArray("body").size() != 0 || payloadMember(bindings) != null)
            } catch (error: IOException) {
                if (!op.flag("safe") || attempt >= config.maxAttempts) throw NetworkError(error)
                delay(retryDelay(attempt, null)); continue
            }
            if (response.status in 200..299) return@withTimeout decodeSuccess(op, response)
            val error = decodeError(op, response)
            val retry = error.throttling || op.flag("safe") && (error.retryable || response.status in setOf(500, 502, 503, 504))
            if (!retry || attempt >= config.maxAttempts) throw error
            delay(retryDelay(attempt, response.header("retry-after")))
        }
        @Suppress("UNREACHABLE_CODE") error("unreachable")
    }
    private suspend fun exchange(request: okhttp3.Request, hasBody: Boolean): WireResponse = suspendCancellableCoroutine { continuation ->
        val call = http.newCall(request)
        continuation.invokeOnCancellation { call.cancel() }
        call.enqueue(object : Callback {
            override fun onFailure(call: Call, error: IOException) { if (continuation.isActive) continuation.resumeWithException(error) }
            override fun onResponse(call: Call, response: Response) {
                response.use {
                    try {
                        val bytes = if (response.isSuccessful && !hasBody) byteArrayOf() else response.body?.bytes() ?: byteArrayOf()
                        val headers = response.headers.names().associate { it.lowercase(Locale.ROOT) to response.headers.values(it) }
                        if (continuation.isActive) continuation.resume(WireResponse(response.code, headers, bytes))
                    } catch (error: IOException) { if (continuation.isActive) continuation.resumeWithException(error) }
                }
            }
        })
    }
    private fun decodeSuccess(op: JsonObject, response: WireResponse): Any {
        val payload = payloadMember(op.getAsJsonObject("response"))
        val body = if (payload != null) JsonObject().also { it.addProperty(payload, Base64.getEncoder().encodeToString(response.body)) }
            else if (response.body.isEmpty()) JsonObject() else codec.parse(String(response.body, Charsets.UTF_8)).asJsonObject
        val target = op.text("output") ?: return kotlin.Unit
        applyHeaders(target, body, op.getAsJsonObject("response").getAsJsonArray("headers").toList(), response)
        return codec.decode(target, body)!!
    }
    private fun applyHeaders(target: String, body: JsonObject, bindings: List<JsonElement>, response: WireResponse) {
        bindings.forEach {
            val pair = it.asJsonArray; val name = pair[0].asString
            response.header(pair[1].asString)?.let { value -> body.add(name, codec.header(codec.shape(target).getAsJsonObject("members").getAsJsonObject(name), value)) }
        }
    }
    private fun decodeError(op: JsonObject, response: WireResponse): ServiceError {
        val body = try { codec.parse(String(response.body, Charsets.UTF_8)).let { if (it.isJsonObject) it.asJsonObject else JsonObject() } } catch (_: RuntimeException) { JsonObject() }
        fun text(name: String): String? = body.get(name)?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isString }?.asString
        var name = text("__type")
        val declared = op.getAsJsonArray("errors").map { it.asString }
        val target = if (name != null) declared.firstOrNull { codec.shape(it).text("name") == name } else
            declared.filter { codec.shape(it).getAsJsonObject("error").get("status").asInt == response.status }.singleOrNull()
        if (name == null && target != null) name = codec.shape(target).text("name")
        val metadata = target?.let { codec.shape(it).getAsJsonObject("error") }
        // A malformed modeled payload must not erase the HTTP failure or its retry traits.
        val error = if (target == null) ServiceError() else try {
            applyHeaders(target, body, metadata!!.getAsJsonArray("headers").toList(), response)
            codec.decode(target, body) as ServiceError
        } catch (_: RuntimeException) { ServiceError() }
        error.initialize(name ?: "UnknownError", text("message") ?: "HTTP ${response.status}", response.status,
            response.header("x-request-id") ?: text("requestId"), response.headers, metadata?.flag("retryable") == true, metadata?.flag("throttling") == true)
        return error
    }
    internal fun retryDelay(attempt: Int, retryAfter: String?): Long {
        if (retryAfter != null) {
            try {
                val seconds = BigDecimal(retryAfter)
                if (seconds.signum() >= 0) return seconds.movePointRight(3).min(BigDecimal.valueOf(Long.MAX_VALUE)).toLong()
            } catch (_: NumberFormatException) { }
            try {
                val remaining = Duration.between(Instant.now(), ZonedDateTime.parse(retryAfter, DateTimeFormatter.RFC_1123_DATE_TIME).toInstant()).toMillis()
                if (remaining >= 0) return remaining
            } catch (_: RuntimeException) { }
        }
        return (Random.nextDouble() * minOf(config.maxDelay.toMillis().toDouble(), config.baseDelay.toMillis() * Math.pow(2.0, minOf(attempt - 1, 30).toDouble()))).toLong()
    }
    fun pages(name: String, input: Any?): Flow<Any> = flow {
        val op = operation(name); val pagination = op.getAsJsonObject("pagination")
        val values = if (input == null) JsonObject() else codec.encode(op.text("input"), input).asJsonObject
        val key = pagination.text("input")!!; val seen = mutableSetOf<String>()
        values.get(key)?.takeUnless { it.isJsonNull }?.let { seen.add(it.asString) }
        while (true) {
            val page = send(prepare(name, values)); emit(page)
            var token: JsonElement? = codec.encode(op.text("output"), page)
            pagination.getAsJsonArray("output").forEach { part -> token = token?.takeIf { it.isJsonObject }?.asJsonObject?.get(part.asString) }
            val next = token?.takeUnless { it.isJsonNull }?.asString
            if (next.isNullOrEmpty()) break
            check(seen.add(next)) { "Pagination token cycle" }
            values.addProperty(key, next)
        }
    }
    companion object {
        private fun payloadMember(bindings: JsonObject): String? = bindings.get("payload")?.takeIf { it.isJsonArray }?.asJsonArray?.get(0)?.asString
        private fun loadSchema(): JsonObject = ClientRuntime::class.java.getResourceAsStream("schema.json")!!.bufferedReader(Charsets.UTF_8).use {
            com.google.gson.JsonParser.parseReader(it).asJsonObject
        }
        private fun escape(value: String): String = URLEncoder.encode(value, "UTF-8").replace("+", "%20").replace("%7E", "~").replace("*", "%2A")
        private fun secure(uri: URI): Boolean = uri.scheme == "https" || uri.scheme == "http" && uri.host.lowercase(Locale.ROOT) in setOf("localhost", "127.0.0.1", "[::1]", "::1")
    }
}
