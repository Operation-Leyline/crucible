import io.ktor.client.HttpClient
import io.ktor.client.plugins.HttpTimeout
import io.ktor.client.plugins.HttpTimeoutConfig
import io.ktor.client.call.body
import io.ktor.client.request.prepareRequest
import io.ktor.client.request.setBody
import io.ktor.client.statement.bodyAsChannel
import io.ktor.http.HttpMethod
import io.ktor.http.Url
import io.ktor.http.content.OutgoingContent
import kotlin.io.encoding.Base64
import kotlin.math.pow
import kotlin.random.Random
import kotlin.time.Clock
import kotlin.time.Duration.Companion.seconds
import io.ktor.utils.io.ByteWriteChannel
import io.ktor.utils.io.copyTo
import kotlin.time.TimeSource
import kotlin.uuid.Uuid
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.booleanOrNull

internal class ClientRuntime(input: ClientConfig, endpoint: String, private val digest: Boolean,
    private val schema: JsonObject = loadSchema()) : AutoCloseable {
    private val config = input.copy(endpoint = (input.endpoint ?: endpoint).trimEnd('/'),
        headers = input.headers.toMap(), middleware = input.middleware.toList(),
        endpointParameters = input.endpointParameters.toMap(), endpointBuiltins = input.endpointBuiltins.toMap())
    private val ownsHttp = input.httpClient == null
    private val http = input.httpClient ?: HttpClient {
        expectSuccess = false; followRedirects = false
        install(HttpTimeout) { requestTimeoutMillis = HttpTimeoutConfig.INFINITE_TIMEOUT_MS; connectTimeoutMillis = HttpTimeoutConfig.INFINITE_TIMEOUT_MS; socketTimeoutMillis = HttpTimeoutConfig.INFINITE_TIMEOUT_MS }
    }
    private val codec = JsonCodec(schema)
    init {
        require(config.endpoint!!.isNotEmpty() || schema["endpointRules"] != null) { "Absolute HTTP(S) endpoint required" }
        if (config.endpoint!!.isNotEmpty()) require(Endpoints.parseUrl(config.endpoint!!)!=null) { "Absolute HTTP(S) endpoint without credentials, query, or fragment required" }
        require(config.maxAttempts >= 1 && config.timeout.isPositive() && config.timeout.isFinite() &&
            config.timeout.inWholeMilliseconds > 0 && !config.baseDelay.isNegative() && !config.maxDelay.isNegative() &&
            config.baseDelay.isFinite() && config.maxDelay.isFinite()) { "Invalid attempts, timeout, or retry delays" }
    }
    override fun close() { if (ownsHttp) http.close() }
    private data class Prepared(val operation: JsonObject, val uri: Url, val headers: Map<String, String>, val body: ByteArray?, val checksum: Checksums.Plan, val stream: ByteStream?)
    private data class WireResponse(val status: Int, val headers: Map<String, List<String>>, val body: ByteArray, val result: Any? = null) {
        fun header(name: String): String? = headers[name.lowercase()]?.joinToString(", ")
    }
    private fun operation(name: String): JsonObject = schema.obj("operations").obj(name)
    private fun prepare(name: String, originalValues: MutableMap<String, JsonElement>, stream: ByteStream? = null): Prepared {
        val values=originalValues.toMutableMap()
        var op = operation(name); val binding = op.obj("request")
        val checksum=Checksums.prepare(name,op,values,config)
        val wireInput=JsonObject(values.mapKeys { (key,_)->codec.wireName(op.text("input")!!,key) })
        val queryInput=codec.queryValue(op.text("input"),wireInput)
        val resolved=if(schema["endpointRules"]!=null) Endpoints.resolve(schema.obj("endpointRules"),(op["endpointParameters"] as? JsonObject) ?: JsonObject(emptyMap()),queryInput,config.endpointParameters,
            mapOf("AWS::Region" to (config.region?.let(::JsonPrimitive) ?: JsonNull),"SDK::Endpoint" to (config.endpoint?.takeIf { it.isNotEmpty() }?.let(::JsonPrimitive) ?: JsonNull))+config.endpointBuiltins)
            else JsonObject(mapOf("url" to JsonPrimitive(config.endpoint!!),"headers" to JsonObject(emptyMap()),"properties" to JsonObject(emptyMap())))
        var endpoint=resolved.text("url")!!
        if(op["endpoint"]!=null && !config.disableHostPrefix) endpoint=Endpoints.hostPrefix(endpoint,op.obj("endpoint"),queryInput.jsonObject)
        endpoint=endpoint.trimEnd('/')
        op=JsonObject(op+mapOf("resolvedEndpoint" to resolved))
        val formats=(binding["timestampFormats"] as? JsonObject) ?: JsonObject(emptyMap())
        for((member,format) in formats) values[member]?.let { values[member]=codec.boundaryTime(codec.shape(op.text("input")!!).obj("members").obj(member),it,format.jsonPrimitive.content,true) }

        op.text("token")?.let { if (values[it] == null || values[it] == JsonNull) values[it] = JsonPrimitive(Uuid.random().toString()) }
        var uri = op.text("uri")!!
        binding.array("labels").forEach {
            val key = it.jsonPrimitive.content; val value = values[key]
            require(value != null && value != JsonNull && codec.bound(value).isNotEmpty()) { "$key is required" }
            val label = codec.bound(value)
            val greedy = (op["greedyLabels"] as? JsonArray)?.any { it.jsonPrimitive.content == key } == true
            val segments = if (greedy) label.split('/') else listOf(label)
            require(segments.none { it == "." || it == ".." }) { "$key cannot contain a dot segment" }
            uri = uri.replace("{" + key + (if (greedy) "+" else "") + "}", segments.joinToString("/") { escape(it) })
        }
        val query = mutableListOf<String>()
        binding.array("query").forEach {
            val pair = it.jsonArray; val value = values[pair[0].jsonPrimitive.content]
            if (value != null && value != JsonNull) {
                val items = if (value is JsonArray) value.toList() else listOf(value)
                items.forEach { item -> query.add(escape(pair[1].jsonPrimitive.content) + "=" + escape(item.jsonPrimitive.content)) }
            }
        }
        binding.text("queryParams")?.let { member ->
            val reserved = binding.array("query").map { it.jsonArray[1].jsonPrimitive.content }.toSet() + Url(endpoint + uri).parameters.names()
            (values[member] as? JsonObject)?.forEach { (key, value) ->
                if (key !in reserved) {
                    val items = if (value is JsonArray) value.toList() else listOf(value)
                    items.forEach { query.add(escape(key) + "=" + escape(codec.bound(it))) }
                }
            }
        }
        if (query.isNotEmpty()) uri += (if ('?' in uri) "&" else "?") + query.joinToString("&")
        val headers = config.headers.mapKeys { it.key.lowercase() }.toMutableMap()
        resolved.obj("headers").forEach { (name,entries)->headers[name.lowercase()]=entries.jsonArray.joinToString(", ") { it.jsonPrimitive.content } }
        val reservedHeaders = binding.array("headers").map { it.jsonArray[1].jsonPrimitive.content.lowercase() }.toSet()
        (binding["prefixHeaders"] as? JsonArray)?.forEach {
            val pair = it.jsonArray
            (values[pair[0].jsonPrimitive.content] as? JsonObject)?.forEach { (suffix, value) ->
                val name = (pair[1].jsonPrimitive.content + suffix).lowercase()
                if (name !in reservedHeaders) headers[name] = codec.bound(headerEncoding(binding, pair[0].jsonPrimitive.content, value, true))
            }
        }
        binding.array("headers").forEach {
            val pair = it.jsonArray; val value = values[pair[0].jsonPrimitive.content]
            if (value != null && value != JsonNull) headers[pair[1].jsonPrimitive.content.lowercase()] = codec.headerText(codec.shape(op.text("input")!!).obj("members").obj(pair[0].jsonPrimitive.content),headerEncoding(binding, pair[0].jsonPrimitive.content, value, true),formats.text(pair[0].jsonPrimitive.content))
        }
        var bytes: ByteArray? = null
        if (binding.array("body").isNotEmpty()) {
            val body = binding.array("body").mapNotNull { field ->
                val key = field.jsonPrimitive.content; values[key]?.let { codec.wireName(op.text("input")!!, key) to it }
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
                if (raw.isNotEmpty() || binding.text("payloadKind") == "string") { bytes = raw; headers["content-type"] = payload[1].jsonPrimitive.content }
            }
        }
        val protocol=schema.text("protocol")
        if(protocol=="awsJson1_0" || protocol=="awsJson1_1") {
            if(bytes==null) bytes="{}".encodeToByteArray()
            headers["content-type"]=if(protocol=="awsJson1_0") "application/x-amz-json-1.0" else "application/x-amz-json-1.1"
            headers["x-amz-target"]=schema.text("service")!!.substringAfter('#')+"."+name
        } else if(protocol=="awsRestJson1" && payloadMember(binding)==null) if(!headers.containsKey("content-type")) headers["content-type"]="application/json"
        return Prepared(op, Url(endpoint + uri), headers, if(stream==null)bytes else null,checksum,stream?.let {checkedStream(op,"input","request",it)})
    }
    suspend fun call(name: String, input: Any?): Any {
        currentCoroutineContext().ensureActive()
        val values = codec.inputValues(operation(name).text("input"), input)
        return send(prepare(name, values, ModelCodecs.stream(operation(name).text("input"),input)))
    }
    private data class WaiterOutcome(val value: Any?)
    suspend fun waitUntil(operation: String, name: String, input: Any?, options: WaiterOptions): Any? {
        val op=this.operation(operation); val description=op.obj("waiters").obj(name)
        val minimum=options.minDelay ?: description.text("minDelay")!!.toLong().seconds
        val maximum=options.maxDelay ?: description.text("maxDelay")!!.toLong().seconds
        require(options.timeout.isPositive() && options.timeout.isFinite() && !minimum.isNegative() && maximum>=minimum && maximum.isFinite()) { "Invalid waiter timeout or delay bounds" }
        val values=codec.inputValues(op.text("input"),input)
        val queryInput=codec.queryValue(op.text("input"),codec.encode(op.text("input"),input))
        var attempts=0; var lastResponse:Any?=null; var lastError:Exception?=null
        val started=TimeSource.Monotonic.markNow()
        val outcome = withTimeoutOrNull(options.timeout) {
            while (true) {
                currentCoroutineContext().ensureActive(); attempts++
                var operationError:Exception?=null
                val operationResponse=try { send(prepare(operation,values.toMutableMap())) }
                    catch (error:TimeoutCancellationException) { currentCoroutineContext().ensureActive(); operationError=error; null }
                    catch (error:CancellationException) { throw error }
                    catch (error:Exception) { operationError=error; null }
                lastResponse=operationResponse; lastError=operationError
                val output=if (lastError==null) codec.queryValue(op.text("output"),codec.encode(op.text("output"),lastResponse)) else JsonNull
                var state=if (lastError==null) "retry" else "failure"; var message:JsonElement?=null
                for (item in description.array("acceptors")) {
                    val acceptor=item.jsonObject; val matcher=acceptor.obj("matcher")
                    val matches=when (matcher.text("kind")) {
                        "success" -> (lastError==null)==matcher.flag("expected")
                        "errorType" -> (lastError as? ServiceError)?.errorName?.let { it==matcher.text("name") || it==matcher.text("name")!!.substringAfterLast('#') } == true
                        else -> if (lastError!=null) false else {
                            val context=if (matcher.text("kind")=="inputOutput") JsonObject(mapOf("input" to queryInput,"output" to output)) else output
                            val result=Query.evaluate(matcher.obj("path").obj("expression"),context); val expected=matcher.text("expected")!!
                            fun stringMatch(value:JsonElement)=value is JsonPrimitive && value.isString && value.content==expected
                            when (matcher.text("comparator")) {
                                "stringEquals" -> stringMatch(result)
                                "booleanEquals" -> result is JsonPrimitive && !result.isString && result.booleanOrNull==(expected=="true")
                                "allStringEquals" -> result is JsonArray && result.isNotEmpty() && result.all { stringMatch(it) }
                                "anyStringEquals" -> result is JsonArray && result.any { stringMatch(it) }
                                else -> error("Unknown waiter comparator")
                            }
                        }
                    }
                    if (matches) {
                        state=acceptor.text("state")!!
                        (acceptor["message"] as? JsonObject)?.let { message=Query.evaluate(it.obj("expression"),output).takeIf { value -> value is JsonPrimitive && value.isString || value is JsonArray && value.all { it is JsonPrimitive && it.isString } } }
                        break
                    }
                }
                if (state=="success") return@withTimeoutOrNull WaiterOutcome(lastResponse)
                if (state=="failure") throw WaiterError(name,attempts,lastResponse,lastError,message)
                val ceiling=minOf(maximum.inWholeMilliseconds.toDouble(),minimum.inWholeMilliseconds.toDouble()*2.0.pow(minOf(attempts-1,62)))
                val chosen=minimum.inWholeMilliseconds.toDouble()+Random.nextDouble()*(ceiling-minimum.inWholeMilliseconds)
                val remaining=options.timeout-started.elapsedNow()
                val sleep=if (remaining.inWholeMilliseconds-chosen<=minimum.inWholeMilliseconds) remaining.inWholeMilliseconds else chosen.toLong()
                delay(maxOf(0,sleep))
            }
            @Suppress("UNREACHABLE_CODE") error("unreachable")
        } ?: throw WaiterTimeout(name,attempts,lastResponse,lastError)
        return outcome.value
    }
    private data class ScopedResult(val value: Any?)
    private class ScopedFailure(val failure: Throwable): RuntimeException(failure)
    @Suppress("UNCHECKED_CAST")
    suspend fun <T> callScoped(name: String,input: Any?,consume: suspend (Any) -> T): T {
        val op=operation(name)
        val prepared=prepare(name,codec.inputValues(op.text("input"),input),ModelCodecs.stream(op.text("input"),input))
        return (send(prepared) { ScopedResult(consume(it)) } as ScopedResult).value as T
    }
    private fun checkedStream(op: JsonObject,targetKey: String,bindingKey: String,stream: ByteStream,verifier: Checksums.Verifier? = null): ByteStream {
        val bindings=op.obj(bindingKey);val member=codec.shape(op.text(targetKey)!!).obj("members").obj(payloadMember(bindings)!!)
        val length=member["length"] as? JsonObject
        fun bound(name:String)=(length?.get(name) as? JsonPrimitive)?.takeUnless {it==JsonNull}?.content?.toLong()
        // requiresLength constrains what the client sends; a response length is checked against its header.
        return stream.checked(bound("min"),bound("max"),bindingKey=="request" && bindings.flag("requiresLength"),verifier)
    }
    private suspend fun send(prepared: Prepared, consume: suspend (Any) -> Any = {it}): Any = withTimeout(config.timeout) {
        val op = prepared.operation
        var attempt = 0
        var streamChecksum: Pair<ByteStream, Map<String, String>>? = null
        while (true) {
            currentCoroutineContext().ensureActive(); attempt++
            val headers = prepared.headers.toMutableMap()
            val request = Request(prepared.uri, op.text("method")!!, headers, prepared.body?.copyOf(),prepared.stream)
            config.middleware.forEach { it(request) }
            require(valid(request.uri)) { "Invalid middleware URI" }
            val body = request.body?.copyOf()
            if (digest && (body?.isNotEmpty() == true || request.method in setOf("POST", "PUT", "PATCH")))
                request.headers["x-amz-content-sha256"] = if(request.stream!=null) requireNotNull(request.stream!!.sha256) { "Streaming content digest requires precomputed SHA-256" } else sha256(body ?: byteArrayOf())
            val stream = request.stream
            check(stream?.openable != false) { "One-shot byte stream has already been opened" }
            if (stream != null) {
                // A replayable body is checksummed once per call, not once per attempt.
                val supplied = streamChecksum?.takeIf { it.first === stream }?.second
                    ?: Checksums.requestStream(prepared.checksum, request.headers, stream).also { streamChecksum = stream to it }
                request.headers.putAll(supplied)
            } else Checksums.request(prepared.checksum,request.headers,body)
            val authenticated = applyAuth(config, schema, op, request)
            val finalHeaders = request.headers.mapKeys { it.key.lowercase() }.toMutableMap()
            require(!(authenticated || finalHeaders.containsKey("authorization")) || secure(request.uri)) { "Credentials require HTTPS or loopback HTTP" }
            currentCoroutineContext().ensureActive()
            val response = try {
                val bindings = op.obj("response")
                exchange(request, finalHeaders, body, bindings.array("body").isNotEmpty() || payloadMember(bindings) != null || prepared.checksum.validate,prepared,consume)
            } catch (error: ScopedFailure) { throw error.failure
            } catch (error: CancellationException) { throw error
            } catch (error: IllegalArgumentException) { throw error
            } catch (error: Exception) {
                // A body that violated its declared or modeled length is a caller error, never retried.
                stream?.failure?.let { throw it }
                if (!op.flag("safe") || attempt >= config.maxAttempts || stream?.replayable==false) throw NetworkError(error)
                delay(retryDelay(attempt, null)); continue
            }
            if (response.status in 200..299) {
                if(response.result!=null)return@withTimeout response.result
                Checksums.validate(prepared.checksum,response.headers,response.body,config)
                return@withTimeout consume(decodeSuccess(op, response))
            }
            val error = decodeError(op, response)
            val retry = error.throttling || op.flag("safe") && (error.retryable || response.status in setOf(500, 502, 503, 504))
            if (!retry || attempt >= config.maxAttempts || stream?.replayable==false) throw error
            delay(retryDelay(attempt, response.header("retry-after")))
        }
        @Suppress("UNREACHABLE_CODE") error("unreachable")
    }
    private suspend fun exchange(request: Request, headers: Map<String, String>, bytes: ByteArray?, hasBody: Boolean, prepared: Prepared, consume: suspend (Any) -> Any): WireResponse =
        http.prepareRequest(request.uri) {
            method = HttpMethod.parse(request.method)
            headers.forEach { (name, value) -> this.headers.append(name, value) }
            val payload = bytes ?: if (request.method in setOf("POST", "PUT", "PATCH")) byteArrayOf() else null
            val stream=request.stream
            if(stream!=null) setBody(object : OutgoingContent.WriteChannelContent() {
                override val contentLength: Long? = stream.contentLength
                override suspend fun writeTo(channel: ByteWriteChannel) {
                    val source=stream.openChannel()
                    try {source.copyTo(channel)} catch(error: Exception) {throw stream.failure ?: error} finally{source.cancel(null)}
                }
            }) else if (payload != null) setBody(object : OutgoingContent.ByteArrayContent() {
                override fun bytes(): ByteArray = payload
                override val contentLength: Long = payload.size.toLong()
            })
        }.execute { response ->
            val responseHeaders=response.headers.entries().associate {it.key.lowercase() to it.value.toList()}
            if(response.status.value in 200..299 && prepared.operation.obj("response").flag("streaming")) {
                // The body stays on the wire until the consumer reads it; the exchange ends with the consumer.
                val length=response.headers["content-length"]?.toLongOrNull()
                val source=ByteStream.once(response.bodyAsChannel(),length)
                var stream: ByteStream?=null
                try {
                    stream=checkedStream(prepared.operation,"output","response",source,Checksums.verifier(prepared.checksum,responseHeaders,config))
                    val wire=WireResponse(response.status.value,responseHeaders,byteArrayOf())
                    val decoded=ModelCodecs.withStream(prepared.operation.text("output")!!,decodeSuccess(prepared.operation,wire),stream)
                    val result=consume(decoded)
                    // A consumer that swallowed a checksum or length failure still receives no result.
                    stream.failure?.let { throw it }
                    return@execute wire.copy(result=result)
                } catch(error: CancellationException) {throw error
                } catch(error: Throwable) {throw ScopedFailure(stream?.failure ?: error)}
                finally {stream?.close();source.close()}
            }
            val body = if (response.status.value in 200..299 && !hasBody) {
                response.bodyAsChannel().cancel(null); byteArrayOf()
            } else response.body<ByteArray>()
            WireResponse(response.status.value, response.headers.entries().associate { it.key.lowercase() to it.value.toList() }, body)
        }
    private fun decodeSuccess(op: JsonObject, response: WireResponse): Any {
        val payload = payloadMember(op.obj("response"))
        val body = if (payload != null) mutableMapOf<String, JsonElement>(codec.wireName(op.text("output")!!, payload) to payloadValue(op.obj("response"), response.body))
            else if (response.body.isEmpty()) mutableMapOf() else codec.parse(response.body.decodeToString()).jsonObject.toMutableMap()
        val target = op.text("output") ?: return kotlin.Unit
        applyHeaders(target, body, op.obj("response"), response)
        return codec.decode(target, JsonObject(body))!!
    }
    private fun applyHeaders(target: String, body: MutableMap<String, JsonElement>, bindings: JsonObject, response: WireResponse) {
        (bindings["prefixHeaders"] as? JsonArray)?.forEach {
            val pair = it.jsonArray; val name = pair[0].jsonPrimitive.content; val prefix = pair[1].jsonPrimitive.content
            val member = codec.shape(target).obj("members").obj(name)
            val mapValue = codec.shape(member.text("target")!!).obj("value")
            val format=(bindings["timestampFormats"] as? JsonObject)?.text(name)
            body[codec.wireName(target, name)] = codec.boundaryTime(member,headerEncoding(bindings, name, JsonObject(response.headers.filterKeys { it.startsWith(prefix, ignoreCase=true) }.map { (key, values) ->
                key.drop(prefix.length) to codec.header(mapValue, values.joinToString(", "),format)
            }.toMap()), false),format,false)
        }
        bindings.text("responseCode")?.let { body[codec.wireName(target, it)] = JsonPrimitive(response.status) }
        bindings.array("headers").forEach {
            val pair = it.jsonArray; val name = pair[0].jsonPrimitive.content
            response.header(pair[1].jsonPrimitive.content)?.let { value ->
                val member=codec.shape(target).obj("members").obj(name); val format=(bindings["timestampFormats"] as? JsonObject)?.text(name)
                body[codec.wireName(target, name)] = codec.boundaryTime(member,headerEncoding(bindings, name, codec.header(member, value, format), false),format,false)
            }
        }
    }
    private fun decodeError(op: JsonObject, response: WireResponse): ServiceError {
        val body = try { (codec.parse(response.body.decodeToString()) as? JsonObject)?.toMutableMap() ?: mutableMapOf() } catch (_: Exception) { mutableMapOf() }
        fun text(name: String): String? = (body[name] as? JsonPrimitive)?.takeIf { it.isString }?.content
        val aws=schema.text("protocol")!=null
        var name = if(aws) response.header("x-amzn-errortype") ?: text("__type") ?: text("code") else text("__type") ?: response.header("x-crucible-error")
        if(aws) name=name?.substringBefore(':')?.substringAfter('#')
        val declared = op.array("errors").map { it.jsonPrimitive.content }
        val target = if (name != null) declared.firstOrNull { (codec.shape(it).obj("error").text("wireCode") ?: codec.shape(it).text("name")) == name } else
            declared.filter { codec.shape(it).obj("error").text("status")?.toInt() == response.status }.singleOrNull()
        if (name == null && target != null) name = codec.shape(target).text("name")
        val metadata = target?.let { codec.shape(it).obj("error") }
        val error = if (target == null) ServiceError() else try {
            payloadMember(metadata!!)?.let { body[codec.wireName(target, it)] = payloadValue(metadata, response.body) }
            applyHeaders(target, body, metadata!!, response)
            codec.decode(target, JsonObject(body)) as ServiceError
        } catch (_: Exception) { ServiceError() }
        error.initialize(name ?: "UnknownError", text("message") ?: text("Message") ?: "HTTP ${response.status}", response.status,
            response.header("x-amzn-requestid") ?: response.header("x-amz-request-id") ?: response.header("x-request-id") ?: text("requestId"), response.headers, metadata?.flag("retryable") == true, metadata?.flag("throttling") == true)
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
    internal fun retryDelay(attempt: Int, retryAfter: String?): Long {
        if (retryAfter != null) {
            retryAfter.toDoubleOrNull()?.takeIf { it >= 0 && it.isFinite() }?.let { return (it * 1000).toLong() }
            try {
                val remaining = (parseHttpDate(retryAfter) - Clock.System.now()).inWholeMilliseconds
                if (remaining >= 0) return remaining
            } catch (_: Exception) { }
        }
        return (Random.nextDouble() * minOf(config.maxDelay.inWholeMilliseconds.toDouble(), config.baseDelay.inWholeMilliseconds * 2.0.pow(minOf(attempt - 1, 30)))).toLong()
    }
    fun pages(name: String, input: Any?): Flow<Any> = flow {
        val op = operation(name); val pagination = op.obj("pagination")
        val values = codec.inputValues(op.text("input"), input)
        val key = pagination.text("input")!!; val seen = mutableSetOf<String>()
        values[key]?.takeUnless { it == JsonNull }?.let { seen.add(it.jsonPrimitive.content) }
        while (true) {
            val page = send(prepare(name, values)); emit(page)
            var token: JsonElement? = codec.encode(op.text("output"), page)
            var target = op.text("output")!!
            pagination.array("output").forEach { part ->
                val name = part.jsonPrimitive.content
                token = (token as? JsonObject)?.get(codec.wireName(target, name))
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
