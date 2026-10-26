import io.ktor.client.HttpClient
import io.ktor.client.plugins.HttpTimeout
import io.ktor.client.plugins.HttpTimeoutConfig
import io.ktor.client.call.body
import io.ktor.client.request.prepareRequest
import io.ktor.client.request.setBody
import io.ktor.client.statement.bodyAsChannel
import io.ktor.http.HttpMethod
import io.ktor.http.HttpHeaders
import io.ktor.http.Url
import io.ktor.http.content.OutgoingContent
import kotlin.io.encoding.Base64
import kotlin.time.Clock
import kotlin.time.Duration
import kotlin.time.Duration.Companion.seconds
import io.ktor.client.network.sockets.ConnectTimeoutException
import io.ktor.client.network.sockets.SocketTimeoutException
import io.ktor.client.plugins.HttpRequestTimeoutException
import kotlin.uuid.Uuid
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.cancel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

internal class ClientRuntime(input: ClientConfig, endpoint: String,
    private val schema: JsonObject = loadSchema()) : AutoCloseable {
    private val config = input.copy(endpoint = (input.endpoint ?: endpoint).trimEnd('/'),
        headers = input.headers.toMap(), middleware = input.middleware.toList())
    private val ownsHttp = input.httpClient == null
    private val http = input.httpClient ?: HttpClient {
        expectSuccess = false; followRedirects = false
        install(HttpTimeout) { requestTimeoutMillis = HttpTimeoutConfig.INFINITE_TIMEOUT_MS; connectTimeoutMillis = HttpTimeoutConfig.INFINITE_TIMEOUT_MS; socketTimeoutMillis = HttpTimeoutConfig.INFINITE_TIMEOUT_MS }
    }
    private val codec = JsonCodec(schema)
    private val clock = config.runtimeClock
    // One quota and rate limiter for every call this client makes.
    private val retry = RetryStrategy(config.retryMode, clock, config.baseDelay, config.maxDelay)
    val retryQuota: RetryQuota? get() = retry.quota
    private val service = schema.text("service") ?: ""
    init {
        require(config.endpoint!!.isNotEmpty() && Endpoints.valid(config.endpoint!!)) { "Absolute HTTP(S) endpoint without credentials, query, or fragment required" }
        require(config.maxAttempts >= 1 && config.timeout.isPositive() && config.timeout.isFinite() &&
            config.timeout.inWholeMilliseconds > 0 && !config.baseDelay.isNegative() && !config.maxDelay.isNegative() &&
            config.baseDelay.isFinite() && config.maxDelay.isFinite()) { "Invalid attempts, timeout, or retry delays" }
    }
    override fun close() { if (ownsHttp) http.close() }
    private class Prepared(val operation: JsonObject, val uri: Url, val headers: Map<String, String>, val body: ByteArray?,
        val endpoint: String, val displayPath: String, val sensitiveHeaders: Set<String>, val sensitiveQuery: Set<String>)
    private class AttemptState { var status: Int? = null; var headers: Map<String, List<String>> = emptyMap(); var bytes: Long? = null }
    private class RetryDecision(val reason: String, val delay: Duration, val cost: Int?)
    private class WireResponse(val status: Int, val headers: Map<String, List<String>>, val body: ByteArray) {
        fun header(name: String): String? = headers[name.lowercase()]?.joinToString(", ")
    }
    private fun operation(name: String): JsonObject = schema.obj("operations").obj(name)
    private fun prepare(name: String, originalValues: MutableMap<String, JsonElement>): Prepared {
        val values=originalValues.toMutableMap()
        val op = operation(name); val binding = op.obj("request")
        val endpoint=config.endpoint!!
        // Telemetry shows where sensitive members went without their values.
        val inputMembers=op.text("input")?.let { codec.shape(it)["members"] as? JsonObject }
        fun sensitive(member: String)=(inputMembers?.get(member) as? JsonObject)?.flag("sensitive")==true
        val sensitiveHeaders=mutableSetOf<String>(); val sensitiveQuery=mutableSetOf<String>()
        val formats=(binding["timestampFormats"] as? JsonObject) ?: JsonObject(emptyMap())
        for((member,format) in formats) values[member]?.let { values[member]=codec.boundaryTime(codec.shape(op.text("input")!!).obj("members").obj(member),it,format.jsonPrimitive.content,true) }

        op.text("token")?.let { if (values[it] == null || values[it] == JsonNull) values[it] = JsonPrimitive(Uuid.random().toString()) }
        var uri = op.text("uri")!!
        var display = uri
        binding.array("labels").forEach {
            val key = it.jsonPrimitive.content; val value = values[key]
            require(value != null && value != JsonNull && codec.bound(value).isNotEmpty()) { "$key is required" }
            val label = codec.bound(value)
            val greedy = (op["greedyLabels"] as? JsonArray)?.any { it.jsonPrimitive.content == key } == true
            val segments = if (greedy) label.split('/') else listOf(label)
            require(segments.none { it == "." || it == ".." }) { "$key cannot contain a dot segment" }
            val placeholder = "{" + key + (if (greedy) "+" else "") + "}"
            val encoded = segments.joinToString("/") { escape(it) }
            uri = uri.replace(placeholder, encoded)
            display = display.replace(placeholder, if (sensitive(key)) CallTrace.REDACTED else encoded)
        }
        val query = mutableListOf<String>()
        binding.array("query").forEach {
            val pair = it.jsonArray; val value = values[pair[0].jsonPrimitive.content]
            if (sensitive(pair[0].jsonPrimitive.content)) sensitiveQuery.add(pair[1].jsonPrimitive.content.lowercase())
            if (value != null && value != JsonNull) {
                val items = if (value is JsonArray) value.toList() else listOf(value)
                items.forEach { item -> query.add(escape(pair[1].jsonPrimitive.content) + "=" + escape(item.jsonPrimitive.content)) }
            }
        }
        if (query.isNotEmpty()) uri += (if ('?' in uri) "&" else "?") + query.joinToString("&")
        val headers = config.headers.mapKeys { it.key.lowercase() }.toMutableMap()
        // A Content-Type bound to an input member outranks the payload's media
        // type, which outranks client header defaults.
        var modelledContentType = false
        binding.array("headers").forEach {
            val pair = it.jsonArray; val value = values[pair[0].jsonPrimitive.content]
            if (sensitive(pair[0].jsonPrimitive.content)) sensitiveHeaders.add(pair[1].jsonPrimitive.content.lowercase())
            if (value != null && value != JsonNull && pair[1].jsonPrimitive.content.lowercase() == "content-type") modelledContentType = true
            if (value != null && value != JsonNull) headers[pair[1].jsonPrimitive.content.lowercase()] = codec.headerText(codec.shape(op.text("input")!!).obj("members").obj(pair[0].jsonPrimitive.content),headerEncoding(binding, pair[0].jsonPrimitive.content, value, true),formats.text(pair[0].jsonPrimitive.content))
        }
        var bytes: ByteArray? = null
        if (binding.array("body").isNotEmpty()) {
            val body = binding.array("body").mapNotNull { field ->
                val key = field.jsonPrimitive.content; values[key]?.let { key to it }
            }.toMap()
            bytes = codec.stringify(JsonObject(body)).encodeToByteArray(); headers["content-type"] = "application/json"
        }
        (binding["payload"] as? JsonArray)?.let { payload ->
            values[payload[0].jsonPrimitive.content]?.takeUnless { it == JsonNull && binding.text("payloadKind") != "document" }?.let { value ->
                val raw = when (binding.text("payloadKind")) {
                    "string" -> value.jsonPrimitive.content.encodeToByteArray()
                    "document" -> codec.stringify(value).encodeToByteArray()
                    else -> Base64.decode(value.jsonPrimitive.content)
                }
                if (raw.isNotEmpty() || binding.text("payloadKind") == "string") { bytes = raw; if (!modelledContentType) headers["content-type"] = payload[1].jsonPrimitive.content }
            }
        }
        // The release this client was generated for (docs/api-versions-clients.md).
        schema.text("apiVersion")?.let { headers["crucible-version"] = it }
        return Prepared(op, Url(endpoint + uri), headers, bytes, endpoint, display.substringBefore('?'), sensitiveHeaders, sensitiveQuery)
    }
    /** The retry reason for a failed attempt, or null when it is not retryable. */
    private fun classify(op: JsonObject, error: Exception): String? {
        if (error !is ServiceError) {
            if (!op.flag("safe")) return null
            return if (error is HttpRequestTimeoutException || error is ConnectTimeoutException || error is SocketTimeoutException) "timeout" else "network"
        }
        if (error.throttling) return "throttling"
        if (op.flag("safe") && (error.retryable || error.status in setOf(500, 502, 503, 504))) return "transient"
        return null
    }
    /** Records a failed attempt with the retry policy and decides whether, and when, to retry. */
    private suspend fun afterFailure(prepared: Prepared, error: Exception, response: WireResponse?, attempt: Int): RetryDecision? {
        retry.afterAttempt(error is ServiceError && error.throttling)
        val reason = classify(prepared.operation, error) ?: return null
        if (attempt >= config.maxAttempts) return null
        val (granted, cost) = retry.acquireRetry(reason == "timeout")
        if (!granted) return null
        return RetryDecision(reason, delayFor(attempt, response?.header("retry-after")), cost)
    }
    /** One call: its telemetry spans preparation, every attempt and the retry sleeps. */
    private suspend fun execute(name: String, prepare: () -> Prepared): Any {
        val trace = CallTrace(config.telemetry, clock, service, name)
        val result = try { send(prepare(), config.timeout, trace) } catch (error: Throwable) { trace.finished(error); throw error }
        trace.finished(null)
        return result
    }
    suspend fun call(name: String, input: Any?): Any {
        currentCoroutineContext().ensureActive()
        val target = operation(name).text("input")
        return execute(name) { prepare(name, codec.inputValues(target, input)) }
    }
    private suspend fun send(prepared: Prepared, timeout: Duration, trace: CallTrace): Any =
        withTimeout(timeout) {
            try { attempts(prepared, trace) }
            catch (error: CancellationException) {
                // Ktor can replace the scope's timeout with a generic call-job cancellation.
                currentCoroutineContext().ensureActive()
                throw error
            }
        }
    private suspend fun attempts(prepared: Prepared, trace: CallTrace): Any {
        val op = prepared.operation
        var lastCost: Int? = null
        for (attempt in 1..config.maxAttempts) {
            currentCoroutineContext().ensureActive()
            val wait = retry.sendDelay()
            if (wait.isPositive()) try { clock.sleep(wait) } catch (error: CancellationException) { retry.cancelSend(); throw error }
            val headers = prepared.headers.toMutableMap()
            val request = Request(prepared.uri, op.text("method")!!, headers, prepared.body?.copyOf())
            config.middleware.forEach { it(request) }
            require(valid(request.uri)) { "Invalid middleware URI" }
            val body = request.body?.copyOf()
            val authenticated = applyAuth(config, op, request)
            val finalHeaders = request.headers.mapKeys { it.key.lowercase() }.toMutableMap()
            require(!(authenticated || finalHeaders.containsKey("authorization")) || secure(request.uri)) { "Credentials require HTTPS or loopback HTTP" }
            currentCoroutineContext().ensureActive()
            val query = request.uri.encodedQuery
            val requestBytes = (body ?: if (request.method in setOf("POST", "PUT", "PATCH")) byteArrayOf() else null)?.size?.toLong()
            trace.attemptStarted(request.method, CallTrace.displayEndpoint(prepared.endpoint),
                prepared.displayPath + if (query.isEmpty()) "" else "?" + CallTrace.redactQuery(query, prepared.sensitiveQuery),
                CallTrace.redactHeaders(finalHeaders, prepared.sensitiveHeaders), requestBytes)
            val state = AttemptState()
            var response: WireResponse? = null
            var failure: Exception? = null
            try {
                try {
                    val bindings = op.obj("response")
                    response = exchange(request, finalHeaders, body, bindings.array("body").isNotEmpty() || payloadMember(bindings) != null, state)
                } catch (error: CancellationException) { throw error
                } catch (error: IllegalArgumentException) { throw error
                } catch (error: Exception) { failure = error }
                val wire = response
                if (wire != null) {
                    if (wire.status in 200..299) {
                        val output = decodeSuccess(op, wire)
                        retry.afterAttempt(false)
                        retry.succeeded(lastCost)
                        trace.attemptFinished(wire.status, null, requestId(wire.headers), state.bytes)
                        return output
                    }
                    failure = decodeError(op, wire)
                }
            } catch (error: Throwable) {
                // An attempt ended by an error that is never retried still measured any response it received.
                if (state.status != null) retry.afterAttempt(false)
                trace.attemptFinished(state.status, error, requestId(state.headers), state.bytes)
                throw error
            }
            val error = failure!!
            val decision = afterFailure(prepared, error, response, attempt)
            trace.attemptFinished(response?.status, error, (error as? ServiceError)?.requestId ?: response?.let { requestId(it.headers) },
                state.bytes, decision?.reason, decision?.delay)
            if (decision == null) throw if (error is ServiceError) error else NetworkError(error)
            lastCost = decision.cost
            clock.sleep(decision.delay)
        }
        error("unreachable")
    }
    private suspend fun exchange(request: Request, headers: Map<String, String>, bytes: ByteArray?, hasBody: Boolean, state: AttemptState): WireResponse =
        http.prepareRequest(request.uri) {
            method = HttpMethod.parse(request.method)
            // Ktor's engine header merge compares these names case-sensitively;
            // lowercase names would be emitted again as content headers.
            headers.forEach { (name, value) -> this.headers.append(when (name) {
                "content-type" -> HttpHeaders.ContentType
                "content-length" -> HttpHeaders.ContentLength
                else -> name
            }, value) }
            val payload = bytes ?: if (request.method in setOf("POST", "PUT", "PATCH")) byteArrayOf() else null
            if (payload != null) setBody(object : OutgoingContent.ByteArrayContent() {
                override fun bytes(): ByteArray = payload
                override val contentLength: Long = payload.size.toLong()
            })
        }.execute { response ->
            val responseHeaders=response.headers.entries().associate {it.key.lowercase() to it.value.toList()}
            state.status=response.status.value; state.headers=responseHeaders
            val body = if (response.status.value in 200..299 && !hasBody) {
                response.bodyAsChannel().cancel(null)
                // CIO can discard a cancelled channel while retaining the response job until EOF.
                response.coroutineContext.cancel()
                byteArrayOf()
            } else response.body<ByteArray>()
            state.bytes=body.size.toLong()
            WireResponse(response.status.value, responseHeaders, body)
        }
    private fun decodeSuccess(op: JsonObject, response: WireResponse): Any {
        val payload = payloadMember(op.obj("response"))
        val body = if (payload != null) mutableMapOf<String, JsonElement>(payload to payloadValue(op.obj("response"), response.body))
            else if (response.body.isEmpty()) mutableMapOf() else codec.parse(response.body.decodeToString()).jsonObject.toMutableMap()
        val target = op.text("output") ?: return kotlin.Unit
        applyHeaders(target, body, op.obj("response"), response)
        return codec.decode(target, JsonObject(body))!!
    }
    private fun applyHeaders(target: String, body: MutableMap<String, JsonElement>, bindings: JsonObject, response: WireResponse) {
        bindings.array("headers").forEach {
            val pair = it.jsonArray; val name = pair[0].jsonPrimitive.content
            response.header(pair[1].jsonPrimitive.content)?.let { value ->
                val member=codec.shape(target).obj("members").obj(name); val format=(bindings["timestampFormats"] as? JsonObject)?.text(name)
                body[name] = codec.boundaryTime(member,headerEncoding(bindings, name, codec.header(member, value, format), false),format,false)
            }
        }
    }
    private fun decodeError(op: JsonObject, response: WireResponse): ServiceError {
        // BEGIN ERROR DOCUMENTATION
        var documentationBodyValid = response.body.isEmpty()
        // END ERROR DOCUMENTATION
        val body = try {
            val parsed = codec.parse(response.body.decodeToString()) as? JsonObject
            // BEGIN ERROR DOCUMENTATION
            documentationBodyValid = response.body.isEmpty() || parsed != null
            // END ERROR DOCUMENTATION
            parsed?.toMutableMap() ?: mutableMapOf()
        } catch (_: Exception) { mutableMapOf() }
        fun text(name: String): String? = (body[name] as? JsonPrimitive)?.takeIf { it.isString }?.content
        var name = text("__type") ?: response.header("x-crucible-error")
        val declared = op.array("errors").map { it.jsonPrimitive.content }
        val target = if (name != null) declared.firstOrNull { (codec.shape(it).obj("error").text("wireCode") ?: codec.shape(it).text("name")) == name } else
            declared.filter { codec.shape(it).obj("error").text("status")?.toInt() == response.status }.singleOrNull()
        if (name == null && target != null) name = codec.shape(target).text("name")
        val metadata = target?.let { codec.shape(it).obj("error") }
        // BEGIN ERROR DOCUMENTATION
        val documentation = metadata?.get("documentation") as? JsonObject
        var documentationUrl = documentation?.text("url")
        // END ERROR DOCUMENTATION
        val error = if (target == null) ServiceException() else try {
            // BEGIN ERROR DOCUMENTATION
            if (payloadMember(metadata!!) == null && !documentationBodyValid) error("modeled error body requires object")
            // END ERROR DOCUMENTATION
            payloadMember(metadata!!)?.let { body[it] = payloadValue(metadata, response.body) }
            applyHeaders(target, body, metadata!!, response)
            val decoded = codec.decode(target, JsonObject(body)) as ServiceError
            // BEGIN ERROR DOCUMENTATION
            val member = documentation?.text("reasonMember")
            val value = member?.let { if (body.containsKey(it)) body[it] else documentation?.get("reasonDefault") }
            val reason = (value as? JsonPrimitive)?.takeIf { it.isString }?.content
            if (reason != null) {
                val pages = documentation?.get("reasonUrls") as? JsonObject
                if (pages?.containsKey(reason) == true) documentationUrl = pages.text(reason)
            }
            // END ERROR DOCUMENTATION
            decoded
        } catch (_: Exception) { ServiceException() }
        error.initialize(name ?: "UnknownError", text("message") ?: "HTTP ${response.status}", response.status,
            response.header("x-request-id") ?: text("requestId"), response.headers, metadata?.flag("retryable") == true, metadata?.flag("throttling") == true)
        // BEGIN ERROR DOCUMENTATION
        error.initializeDocumentation(documentationUrl)
        // END ERROR DOCUMENTATION
        return error
    }
    private fun payloadValue(binding: JsonObject, bytes: ByteArray): JsonElement = when (binding.text("payloadKind")) {
        "string" -> JsonPrimitive(bytes.decodeToString())
        "document" -> codec.parse(bytes.decodeToString())
        else -> JsonPrimitive(Base64.encode(bytes))
    }
    private fun headerEncoding(binding: JsonObject, member: String, value: JsonElement, encode: Boolean): JsonElement {
        if ((binding["headerEncodings"] as? JsonObject)?.text(member) != "base64") return value
        fun convert(item: JsonElement): JsonElement = when (item) {
            is JsonArray -> JsonArray(item.map { convert(it) })
            is JsonObject -> JsonObject(item.mapValues { convert(it.value) })
            else -> JsonPrimitive(if (encode) Base64.encode(item.jsonPrimitive.content.encodeToByteArray()) else Base64.decode(item.jsonPrimitive.content).decodeToString())
        }
        return convert(value)
    }
    private fun requestId(headers: Map<String, List<String>>): String? =
        headers["x-request-id"]?.joinToString(", ")
    private fun delayFor(attempt: Int, retryAfter: String?): Duration {
        if (retryAfter != null) {
            retryAfter.toDoubleOrNull()?.takeIf { it >= 0 && it.isFinite() }?.let { return it.seconds }
            try {
                val remaining = parseHttpDate(retryAfter) - Clock.System.now()
                if (!remaining.isNegative()) return remaining
            } catch (_: Exception) { }
        }
        return retry.backoff(attempt)
    }
    fun pages(name: String, input: Any?): Flow<Any> = flow {
        val op = operation(name); val pagination = op.obj("pagination")
        val values = codec.inputValues(op.text("input"), input)
        val key = pagination.text("input")!!; val seen = mutableSetOf<String>()
        values[key]?.takeUnless { it == JsonNull }?.let { seen.add(it.jsonPrimitive.content) }
        while (true) {
            val page = execute(name) { prepare(name, values) }; emit(page)
            var token: JsonElement? = codec.encode(op.text("output"), page)
            var target = op.text("output")!!
            pagination.array("output").forEach { part ->
                val name = part.jsonPrimitive.content
                token = (token as? JsonObject)?.get(name)
                target = codec.shape(target).obj("members").obj(name).text("target")!!
            }
            val next = token?.takeUnless { it == JsonNull }?.jsonPrimitive?.content
            if (next.isNullOrEmpty()) break
            check(seen.add(next)) { "Pagination token cycle" }
            values[key] = JsonPrimitive(next)
        }
    }
    companion object {
        private fun payloadMember(bindings: JsonObject): String? = (bindings["payload"] as? JsonArray)?.get(0)?.jsonPrimitive?.content
        private fun valid(uri: Url): Boolean = uri.protocol.name in setOf("http", "https") && uri.host.isNotEmpty() && uri.user == null && uri.password == null
        private fun escape(value: String): String = buildString {
            value.encodeToByteArray().forEach { byte ->
                val n = byte.toInt() and 255
                if (n in 65..90 || n in 97..122 || n in 48..57 || n in listOf(45,46,95,126)) append(n.toChar())
                else { append('%'); append("0123456789ABCDEF"[n ushr 4]); append("0123456789ABCDEF"[n and 15]) }
            }
        }
        private fun secure(uri: Url): Boolean = uri.protocol.name == "https" || uri.protocol.name == "http" && uri.host.lowercase() in setOf("localhost", "127.0.0.1", "[::1]", "::1")
    }
}
