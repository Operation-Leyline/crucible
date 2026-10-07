package example.weather

import example.weather.model.BuilderDefaults
import example.weather.model.ValidationError
import example.weather.model.ThrottlingError
import example.weather.model.Sky
import example.weather.model.PutCityPhotoInput
import example.weather.model.Location
import example.weather.model.ListCitiesInput
import example.weather.model.GetCityPhotoInput
import example.weather.model.GetCityInput
import example.weather.model.Forecast
import example.weather.model.CreateCityInput
import example.weather.model.ConflictError
import example.weather.model.CitySummary
import example.weather.model.TransferAlgorithm
import kotlinx.serialization.json.*
import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpServer
import java.io.Closeable
import java.io.IOException
import java.lang.reflect.InvocationTargetException
import java.net.InetSocketAddress
import java.nio.file.Files
import java.nio.file.Paths
import java.security.MessageDigest
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds
import kotlin.time.Instant
import java.util.concurrent.Executors
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import io.ktor.utils.io.readAvailable
import io.ktor.utils.io.writeFully
import io.ktor.utils.io.writer


private val JsonElement?.asJsonObject: JsonObject get() = this!!.jsonObject
private val JsonElement?.asJsonArray: JsonArray get() = this!!.jsonArray
private val JsonElement?.asJsonPrimitive: JsonPrimitive get() = this!!.jsonPrimitive
private val JsonElement?.asString: String get() = this!!.jsonPrimitive.content
private val JsonElement?.asInt: Int get() = asString.toInt()
private val JsonElement?.asLong: Long get() = asString.toLong()
private val JsonElement?.asBoolean: Boolean get() = asString.toBooleanStrict()
private val JsonElement?.asBigDecimal: java.math.BigDecimal get() = java.math.BigDecimal(asString)
private val JsonElement?.isJsonNull: Boolean get() = this == JsonNull
private val JsonElement?.isJsonPrimitive: Boolean get() = this is JsonPrimitive && this != JsonNull
private val JsonElement?.isJsonObject: Boolean get() = this is JsonObject
private val JsonElement?.isJsonArray: Boolean get() = this is JsonArray
private val JsonPrimitive.isNumber: Boolean get() = !isString && booleanOrNull == null && this != JsonNull
private fun JsonObject?.getAsJsonObject(name: String): JsonObject? = this?.get(name) as? JsonObject
private fun JsonObject?.get(name: String): JsonElement? = this?.get(name)
private fun JsonObject.has(name: String): Boolean = containsKey(name)
private fun JsonObject.keySet(): Set<String> = keys
private fun JsonObject.entrySet(): Set<Map.Entry<String, JsonElement>> = entries
private fun JsonArray.size(): Int = size
private fun changed(objectValue: JsonObject, path: List<String>, value: JsonElement): JsonObject = JsonObject(objectValue.toMutableMap().apply {
    this[path.first()] = if (path.size == 1) value else changed(getValue(path.first()).jsonObject, path.drop(1), value)
})

private fun json(value: String): JsonElement = Json.parseToJsonElement(value)
private class Server(handler: (HttpExchange) -> Unit) : Closeable {
    private val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
    private val executor = Executors.newCachedThreadPool { task -> Thread(task).also { it.isDaemon = true } }
    @Volatile var failure: Throwable? = null
    val endpoint get() = "http://127.0.0.1:${server.address.port}"
    init {
        server.executor = executor
        server.createContext("/") { exchange ->
            try { handler(exchange) }
            catch (_: IOException) { }
            catch (_: InterruptedException) { Thread.currentThread().interrupt() }
            catch (error: Throwable) { failure = error }
            finally { exchange.close() }
        }
        server.start()
    }
    fun config() = ClientConfig(endpoint = endpoint, baseDelay = Duration.ZERO)
    override fun close() { server.stop(0); executor.shutdownNow(); failure?.let { throw AssertionError(it) } }
}
private fun response(exchange: HttpExchange, status: Int, body: String) {
    response(exchange, status, body.toByteArray(Charsets.UTF_8))
}
private fun response(exchange: HttpExchange, status: Int, bytes: ByteArray) {
    exchange.sendResponseHeaders(status, bytes.size.toLong())
    exchange.responseBody.write(bytes)
}
private suspend fun retries(path: String) {
    for (case in json(String(Files.readAllBytes(Paths.get(path)))).asJsonArray) {
        val test = case.asJsonObject; val attempts = AtomicInteger()
        Server { exchange ->
            val body = buildJsonObject { if (!test.get("error").isJsonNull) put("__type", test.getValue("error")) }
            exchange.responseHeaders.add("x-amzn-errortype", "Throttled")
            response(exchange, if (attempts.incrementAndGet() <= test.get("failures").asInt) test.get("status").asInt else 200, body.toString())
        }.use { server ->
            WeatherClient(server.config().copy(maxAttempts = test.get("maxAttempts").asInt)).use { client ->
                val success = try {
                    if (test.get("safe").asBoolean) client.retrySafe() else client.retryUnsafe()
                    true
                } catch (_: ServiceError) { false }
                check(attempts.get() == test.get("attempts").asInt && success == test.get("success").asBoolean) { test.get("name").asString }
            }
        }
    }
}
private class Codec {
    private val type = Class.forName("example.weather.JsonCodec")
    private val schema = WeatherClient::class.java.getResourceAsStream("schema.json")!!.bufferedReader().use { json(it.readText()).asJsonObject }
    private var instance = type.getConstructor(JsonObject::class.java).newInstance(schema)
    private fun invoke(name: String, vararg args: Any?): Any? {
        val method = type.methods.first { it.name == name && it.parameterCount == args.size }
        try { return method.invoke(instance, *args) } catch (error: InvocationTargetException) { throw error.cause!! }
    }
    fun decode(target: String, wire: String): Any? = invoke("decode", target, json(wire), null)
    fun encode(target: String, value: Any?, format: String? = null): JsonElement = invoke("encode", target, value, format) as JsonElement
    fun header(wire: String): JsonElement = invoke("header", json("""{"target":"example.weather#Tags"}""").asJsonObject, wire, null) as JsonElement
    fun bound(values: JsonElement): String = invoke("bound", values) as String
    fun checkDenseMapNulls() {
        instance = type.getConstructor(JsonObject::class.java).newInstance(changed(schema, listOf("shapes", "example.weather#Forecast", "value", "target"), JsonPrimitive("smithy.api#String")))
        try { decode("example.weather#Forecast", "{\"key\":null}"); error("Dense map null accepted") } catch (_: IllegalArgumentException) {}
        try { encode("example.weather#Forecast", mapOf("key" to null)); error("Dense map null encoded") } catch (_: IllegalArgumentException) {}
    }
}
private fun serialization(path: String) {
    val codec = Codec()
    for (row in json(Files.readString(Paths.get(path).resolveSibling("serialization.json"))).asJsonArray) {
        val test = row.asJsonObject
        val id = test.get("id").asString; val target = test.get("target").asString
        val value = try { codec.decode(target, test.get("wire").asString) } catch (error: RuntimeException) {
            if (test.get("reject")?.asBoolean == true) continue
            throw AssertionError(id, error)
        }
        check(test.get("reject")?.asBoolean != true) { "$id accepted invalid wire" }
        val expect = test.getAsJsonObject("expect")
        val encoded = codec.encode(target, value)
        check(exactJsonEquals(encoded, json(expect.get("json").asString))) { "$id: $encoded" }
        expect.getAsJsonObject("states")?.entrySet()?.forEach { (member, state) ->
            val field = value!!::class.java.getMethod("get" + member.replaceFirstChar { it.uppercase() }).invoke(value)
            val has = value::class.java.methods.find { it.name == "has" + member.replaceFirstChar { it.uppercase() } }
            val actual = if (has?.invoke(value) == false) "absent" else if (field == null) "null" else "value"
            check(actual == state.asString) { "$id native presence" }
        }
        expect.get("blobHex")?.let { check(java.util.HexFormat.of().formatHex(value as ByteArray) == it.asString) { "$id bytes" } }
        expect.getAsJsonObject("instant")?.let { instant ->
            val member = instant.get("member").asString
            val actual = value!!::class.java.getMethod("get" + member.replaceFirstChar { it.uppercase() }).invoke(value) as Instant
            check(actual.epochSeconds.toString() == instant.get("seconds").asString && actual.nanosecondsOfSecond == instant.get("nanoseconds").asInt) { "$id instant" }
        }
    }
    check(codec.decode("example.weather#Sky", "\"future\"") == Sky("future"))
    val future = """{"future":{"n":9223372036854775807}}"""
    val union = codec.decode("example.weather#Location", future)
    check(union is Location.UnknownVariant && codec.encode("example.weather#Location", union) == json(future))
    check(codec.encode("example.weather#Location", Location.Unknown(kotlin.Unit)) == json("""{"unknown":{}}"""))
    val city = codec.decode("example.weather#CitySummary", """{"id":"c","name":"x","createdAt":-0.000000001,"population":9223372036854775807}""") as CitySummary
    check(city.population == Long.MAX_VALUE && city.createdAt == Instant.fromEpochSeconds(-1, 999999999))
    check((codec.decode("example.weather#CitySummary", """{"id":"c","name":"x"}""") as CitySummary).population == 0L)
    check(CitySummary { id = "c"; name = "x" }.population == 0L)
    val defaults = BuilderDefaults { requiredValue = "required" }
    check(defaults.copy().builder == "builder-value" && defaults.copy().block == "block-value")
    check(defaults.flag && defaults.small == (-7).toByte() && defaults.medium == 300.toShort())
    check(defaults.count == 42 && defaults.large == Long.MIN_VALUE && defaults.ratio == 1.5f && defaults.score == 2.5)
    check(defaults.decimalCount == 17 && defaults.blob.contentEquals(byteArrayOf(1, 2)))
    check(defaults.date == Instant.parse("1969-12-31T23:59:59.750Z") && defaults.epoch == Instant.fromEpochSeconds(-2, 500_000_000))
    check(defaults.label == "default \$value\\path" && defaults.sky == Sky("sunny"))
    check(defaults.tags.isEmpty() && defaults.forecast.isEmpty() && defaults.document.asJsonObject.get("count").asLong == Long.MAX_VALUE)
    check(!defaults.hasOptionalDefault())
    val decodedDefaults = codec.decode("example.weather#BuilderDefaults", """{"requiredValue":"required"}""") as BuilderDefaults
    check(!decodedDefaults.hasOptionalDefault() && decodedDefaults.count == 42 && decodedDefaults.date == defaults.date)
    check(BuilderDefaults { requiredValue = "required"; count = 0 }.count == 0)
    check(!BuilderDefaults { requiredValue = "required" }.document.asJsonObject.has("changed"))
    val reusedDefaults = BuilderDefaults.Builder().apply { requiredValue = "required" }
    val firstDefaults = reusedDefaults.build()
    firstDefaults.blob[0] = 99
    val secondDefaults = reusedDefaults.build()
    check(!secondDefaults.document.asJsonObject.has("changed") && secondDefaults.blob.contentEquals(byteArrayOf(1, 2)))
    check(reusedDefaults.apply { count = 0 }.build().count == 0)
    try { BuilderDefaults {}; error("Missing required member accepted") } catch (_: IllegalArgumentException) { }
    try { BuilderDefaults { requiredValue = "required"; count = null }; error("Explicit null replaced by default") } catch (_: IllegalArgumentException) { }
    val present = CreateCityInput { this.name = "x"; this.location = null }
    check(present.name == "x" && present.hasLocation() && present.location == null)
    val omitted = present.copy { unsetLocation() }
    val listInput = ListCitiesInput { tags = listOf("ordinary", "list") }
    check(!omitted.hasLocation() && present.hasLocation() && listInput.tags == listOf("ordinary", "list"))
    check(!codec.encode("example.weather#CreateCityInput", omitted).asJsonObject.has("location"))
    val encoded = codec.encode("example.weather#CreateCityInput", present).asJsonObject
    check(encoded.get("location").isJsonNull && !encoded.has("clientToken"))
    val map = codec.decode("example.weather#Forecast", """{"future":{"n":9223372036854775807}}""") as Forecast
    check(map[Sky("future")]!!.asJsonObject.get("n").asLong == Long.MAX_VALUE)
    val nullDocument = codec.decode("example.weather#Forecast", """{"future":{"nil":null}}""") as Forecast
    check(nullDocument[Sky("future")]!!.asJsonObject.get("nil").isJsonNull)
    try { codec.decode("example.weather#Forecast", """{"future":null}"""); error("Dense document null accepted") } catch (_: IllegalArgumentException) {}
    try { codec.encode("example.weather#Forecast", mapOf(Sky("future") to JsonNull)); error("Dense document null encoded") } catch (_: IllegalArgumentException) {}
    val binary = byteArrayOf(0, 1, -1, -128)
    check(codec.encode("example.weather#Photo", binary).asString == "AAH/gA==")
    check((codec.decode("example.weather#Photo", "\"AAH/gA==\"") as ByteArray).contentEquals(binary))
    try { codec.decode("example.weather#Tags", "[null]"); error("Null element accepted in a non-sparse list") }
    catch (_: IllegalArgumentException) { }
    for (test in json(String(Files.readAllBytes(Paths.get(path)))).asJsonArray) {
        check(codec.header(test.asJsonObject.get("wire").asString) == test.asJsonObject.get("values"))
        val encoded = codec.bound(test.asJsonObject.getValue("values"))
        check(encoded == test.asJsonObject.get("encoded").asString)
        check(codec.header(encoded) == test.asJsonObject.get("values"))
    }
    check(codec.encode("example.weather#DateTime", Instant.parse("2026-10-01T00:00:00Z"), "http-date").asString == "Thu, 01 Oct 2026 00:00:00 GMT")
}
private fun exactJsonEquals(a: JsonElement, b: JsonElement): Boolean {
    if (a.isJsonPrimitive && a.asJsonPrimitive.isNumber) return b.isJsonPrimitive && b.asJsonPrimitive.isNumber && a.asBigDecimal.compareTo(b.asBigDecimal) == 0
    if (a.isJsonObject) return b.isJsonObject && a.asJsonObject.keySet() == b.asJsonObject.keySet() && a.asJsonObject.entrySet().all { (key, value) -> exactJsonEquals(value, b.asJsonObject.getValue(key)) }
    if (a.isJsonArray) return b.isJsonArray && a.asJsonArray.size() == b.asJsonArray.size() && (0 until a.asJsonArray.size()).all { exactJsonEquals(a.asJsonArray[it], b.asJsonArray[it]) }
    return a == b
}
private suspend fun httpBehavior() {
    val sent = mutableListOf<ByteArray>(); val credentials = AtomicInteger()
    Server { exchange ->
        val bytes = exchange.requestBody.readBytes(); sent.add(bytes)
        val hash = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it.toInt() and 255) }
        check(exchange.requestHeaders.getFirst("x-amz-content-sha256") == hash)
        check(exchange.requestHeaders.getFirst("authorization") == "Bearer ${sent.size}")
        check(exchange.requestHeaders.getFirst("x-expected-revision") == "7")
        response(exchange, if (sent.size == 1) 503 else 200,
            if (sent.size == 1) """{"__type":"FutureError"}""" else """{"city":{"id":"c","name":"x"}}""")
    }.use { server ->
        WeatherClient(server.config().copy(token = { delay(1); credentials.incrementAndGet().toString() },
            middleware = listOf({ request -> delay(1); request.body = request.body!! + byteArrayOf(32) }))).use { client ->
            val input = CreateCityInput { this.name = "x"; this.expectedRevision = 7L }
            client.createCity(input)
            check(!input.hasClientToken() && sent.size == 2 && sent[0].contentEquals(sent[1]))
        }
    }
    Server { exchange ->
        check(exchange.requestURI.rawPath.contains("a%2Fb%20%2A%3F%23%C3%A9", ignoreCase = true))
        exchange.responseHeaders.add("x-request-id", "header")
        response(exchange, 200, """{"city":{"id":"c","name":"x"}}""")
    }.use { server ->
        WeatherClient(server.config()).use { check(it.getCity(GetCityInput { this.cityId = "a/b *?#é" }).requestIdValue == "header") }
    }
    Server { exchange ->
        exchange.responseHeaders.add("x-request-id", "wire")
        response(exchange, 400, """{"__type":"ValidationError","message":"bad","reason":"Invalid","requestId":"body"}""")
    }.use { server ->
        WeatherClient(server.config()).use { client ->
            try { client.getCity(GetCityInput { this.cityId = "x" }); error("missing typed error") }
            catch (error: ValidationError) {
                check(error.requestId == "wire" && error.requestIdValue == "body" && error.status == 400 && error.message == "bad")
                val copied = error.copy()
                check(copied.requestId == error.requestId && copied.status == error.status && copied.message == error.message)
                check(copied.headers == error.headers && copied.retryable == error.retryable && copied.throttling == error.throttling)
            }
        }
    }
    val malformed = AtomicInteger()
    Server { exchange ->
        exchange.responseHeaders.add("x-request-id", "malformed")
        response(exchange, if (malformed.incrementAndGet() == 1) 429 else 200, """{"__type":"Throttled","message":{}}""")
    }.use { server -> WeatherClient(server.config()).use { it.retryUnsafe(); check(malformed.get() == 2) } }
}
private suspend fun failures() = coroutineScope {
    for ((status, body) in listOf(
        429 to """{"__type":"ThrottlingError","message":"throttled"}""",
        499 to """{"__type":"FutureException","message":"future"}""",
        400 to """{"__type":"ValidationError","message":"bad","reason":{}}""")) {
        Server { exchange -> exchange.responseHeaders.add("x-request-id", "base-id"); response(exchange, status, body) }.use { server ->
            WeatherClient(server.config()).use { client ->
                try { client.ping(); error("error response accepted") }
                catch (error: WeatherException) { check(error.status == status && error.requestId == "base-id"); if (status == 429) check(error is ThrottlingError) }
            }
        }
    }

    val count = AtomicInteger()
    Server { exchange -> count.incrementAndGet(); response(exchange, 200, "invalid") }.use { server ->
        WeatherClient(server.config()).use { client ->
            try { client.getCity(GetCityInput { this.cityId = "x" }); error("invalid JSON accepted") }
            catch (_: kotlinx.serialization.SerializationException) { check(count.get() == 1) }
        }
    }
    count.set(0)
    Server { exchange ->
        val body = """{"city":{"id":"c","name":"x"}}""".toByteArray()
        exchange.sendResponseHeaders(200, body.size.toLong() + if (count.incrementAndGet() == 1) 10 else 0)
        exchange.responseBody.write(body)
    }.use { server -> WeatherClient(server.config()).use { it.getCity(GetCityInput { this.cityId = "x" }); check(count.get() == 2) } }
    val headersSent = CompletableDeferred<Unit>()
    val releaseBody = CountDownLatch(1)
    Server { exchange ->
        exchange.sendResponseHeaders(200, 100)
        exchange.responseBody.write('{'.code); exchange.responseBody.flush()
        headersSent.complete(Unit)
        releaseBody.await()
    }.use { server ->
        WeatherClient(server.config()).use { client ->
            try {
                val call = async { client.ping() }
                withTimeout(10000) { headersSent.await(); call.await() }
                check(releaseBody.count == 1L) { "header-only call waited for the body" }
            } finally { releaseBody.countDown() }
        }
    }
    count.set(0)
    Server { exchange -> count.incrementAndGet(); response(exchange, 200, "{}") }.use { server ->
        val entered = CompletableDeferred<Unit>(); val release = CompletableDeferred<String>()
        WeatherClient(server.config().copy(token = { entered.complete(Unit); release.await() })).use { client ->
            val call = launch { client.ping() }; entered.await(); call.cancelAndJoin(); release.complete("token")
            delay(50); check(count.get() == 0)
        }
    }
    WeatherClient(ClientConfig(endpoint = "http://example.invalid", token = { "secret" })).use { client ->
        try { client.ping(); error("plaintext credentials accepted") } catch (_: IllegalArgumentException) { }
    }
    count.set(0)
    Server { exchange -> count.incrementAndGet(); response(exchange, 200, """{"cities":[],"nextToken":"same"}""") }.use { server ->
        WeatherClient(server.config()).use { client ->
            val input = ListCitiesInput {  }
            try { client.listCitiesPages(input).toList(); error("cycle accepted") } catch (_: IllegalStateException) { }
            check(count.get() == 2 && !input.hasNextToken())
        }
    }
}
private suspend fun deadlines() = coroutineScope {
    Server { exchange ->
        Thread.sleep(11000)
        response(exchange, 200, "{}")
    }.use { server ->
        WeatherClient(server.config().copy(timeout = 15.seconds, maxAttempts = 1)).use { client ->
            withTimeout(18000) { client.ping() }
        }
    }
    val attempts = AtomicInteger()
    Server { exchange ->
        attempts.incrementAndGet()
        exchange.sendResponseHeaders(200, 100)
        exchange.responseBody.write('{'.code); exchange.responseBody.flush()
        Thread.sleep(5000)
    }.use { server ->
        WeatherClient(server.config().copy(timeout = 250.milliseconds)).use { client ->
            withTimeout(2000) {
                try { client.getCity(GetCityInput { this.cityId = "x" }); error("deadline ignored") }
                catch (_: TimeoutCancellationException) { }
            }
            check(attempts.get() == 1) { "deadline retried the interrupted exchange" }
        }
    }
    val entered = CompletableDeferred<Unit>()
    Server { exchange ->
        exchange.sendResponseHeaders(200, 100)
        exchange.responseBody.write('{'.code); exchange.responseBody.flush()
        entered.complete(Unit)
        Thread.sleep(5000)
    }.use { server ->
        WeatherClient(server.config()).use { client ->
            val call = launch { client.getCity(GetCityInput { this.cityId = "x" }) }
            withTimeout(2000) { entered.await(); call.cancelAndJoin() }
        }
    }
    val injected = io.ktor.client.HttpClient(io.ktor.client.engine.cio.CIO) { install(io.ktor.client.plugins.HttpTimeout) { socketTimeoutMillis = 100 } }
    try {
        Server { exchange ->
            exchange.sendResponseHeaders(200, 100)
            exchange.responseBody.write('{'.code); exchange.responseBody.flush()
            Thread.sleep(5000)
        }.use { server ->
            WeatherClient(server.config().copy(httpClient = injected, maxAttempts = 1)).use { client ->
                try { withTimeout(2000) { client.getCity(GetCityInput { this.cityId = "x" }) }; error("injected timeout ignored") }
                catch (error: NetworkError) {
                    check(generateSequence(error.cause) { it.cause }.any { it is io.ktor.client.network.sockets.SocketTimeoutException }) {
                        "Unexpected injected transport failure: " + generateSequence(error.cause) { it.cause }.joinToString { "${it::class.qualifiedName}: ${it.message}" }
                    }
                }
            }
            check(injected.coroutineContext[kotlinx.coroutines.Job]!!.isActive) { "caller-owned transport was closed" }
        }
    } finally {
        injected.close()
    }
}
private suspend fun payloads() {
    val binary = byteArrayOf(0, 1, -1, -128, 13, 10)
    val calls = AtomicInteger()
    Server { exchange ->
        val bytes = exchange.requestBody.readBytes()
        check(bytes.contentEquals(binary))
        check(exchange.requestHeaders.getFirst("Content-Type") == "image/jpeg")
        check(exchange.requestHeaders.getFirst("X-Photo-Caption") == "caption")
        val hash = MessageDigest.getInstance("SHA-256").digest(binary).joinToString("") { "%02x".format(it.toInt() and 255) }
        check(exchange.requestHeaders.getFirst("x-amz-content-sha256") == hash)
        response(exchange, if (calls.incrementAndGet() == 1) 503 else 200,
            if (calls.get() == 1) """{"__type":"FutureError"}""" else """{"photo":{"size":6,"thumbnail":"AAH/gA0K"}}""")
    }.use { server -> WeatherClient(server.config()).use { client ->
        val result = client.putCityPhoto(PutCityPhotoInput { this.cityId = "x"; this.caption = "caption"; this.photo = binary })
        check(result.photo.thumbnail.contentEquals(binary) && calls.get() == 2)
    } }
    for (bytes in listOf(binary, byteArrayOf())) {
        Server { exchange -> exchange.responseHeaders.add("X-Photo-Caption", "raw"); response(exchange, 200, bytes) }
            .use { server -> WeatherClient(server.config()).use { client ->
                val result = client.getCityPhoto(GetCityPhotoInput { this.cityId = "x" })
                check(result.hasPhoto() && result.photo!!.contentEquals(bytes) && result.caption == "raw")
            } }
    }
    Server { exchange ->
        check(exchange.requestBody.readBytes().isEmpty() && exchange.requestHeaders.getFirst("Content-Type") == null)
        response(exchange, 200, """{"photo":{"size":0,"thumbnail":""}}""")
    }.use { server -> WeatherClient(server.config()).use { it.putCityPhoto(PutCityPhotoInput { this.cityId = "x"; this.photo = byteArrayOf() }) } }
}
private suspend fun weather() {
    WeatherClient(ClientConfig(endpoint = System.getenv("WEATHER_URL"), token = { "test-token" })).use { client ->
        val input = CreateCityInput { this.name = "kotlin-http"; this.location = Location.Unknown(kotlin.Unit) }
        val city = client.createCity(input)
        check(client.getCity { cityId = city.city.id }.city.name == "kotlin-http")
        val photo = byteArrayOf(0, 1, -1, -128, 13, 10, 65, 66, 67)
        val stored = client.putCityPhoto(PutCityPhotoInput { this.cityId = city.city.id; this.caption = "binary"; this.photo = photo })
        check(stored.photo.size == photo.size && stored.photo.thumbnail.contentEquals(photo.take(8).toByteArray()))
        val fetched = client.getCityPhoto(GetCityPhotoInput { this.cityId = city.city.id })
        check(fetched.photo!!.contentEquals(photo) && fetched.caption == "binary")
        try { client.createCity(input); error("missing conflict") } catch (_: ConflictError) { }
        val pages = client.listCitiesPages { maxResults = 1 }.toList()
        check(pages.any { page -> page.cities.any { it.name == "kotlin-http" } })
    }
}
private suspend fun hardening() {
    val sent = AtomicInteger()
    Server { exchange ->
        sent.incrementAndGet()
        check(exchange.requestHeaders.getFirst("Authorization") == null)
        check(exchange.requestBody.readBytes().isEmpty())
        val hash = if (exchange.requestMethod in listOf("GET", "DELETE")) null else
            MessageDigest.getInstance("SHA-256").digest(byteArrayOf()).joinToString("") { "%02x".format(it.toInt() and 255) }
        check(exchange.requestHeaders.getFirst("x-amz-content-sha256") == hash)
        exchange.sendResponseHeaders(200, -1)
    }.use { server ->
        WeatherClient(server.config().copy(token = { error("Anonymous credential provider invoked") })).use { client ->
            val runtimeField = client.javaClass.getDeclaredField("runtime").apply { isAccessible = true }
            val runtime = runtimeField.get(client)
            val schemaField = runtime.javaClass.getDeclaredField("schema").apply { isAccessible = true }
            val schema = schemaField.get(runtime) as JsonObject
            val anonymous = changed(schema, listOf("operations", "Ping", "bearer"), JsonPrimitive(false))
            for (method in listOf("POST", "PUT", "PATCH", "GET", "DELETE")) {
                schemaField.set(runtime, changed(anonymous, listOf("operations", "Ping", "method"), JsonPrimitive(method)))
                client.ping()
            }
            for (label in listOf(".", "..")) {
                try { client.getCity { cityId = label }; error("Dot label accepted") }
                catch (e: IllegalArgumentException) { check(e.message!!.contains("dot segment")) }
            }
        }
    }
    check(sent.get() == 5)
    val codec = Codec()
    try { codec.decode("example.weather#Tags", "[null]"); error("Dense null accepted") } catch (_: IllegalArgumentException) {}
    try { codec.encode("example.weather#Tags", listOf(null)); error("Dense null encoded") } catch (_: IllegalArgumentException) {}
    check(codec.encode("example.weather#Forecast", codec.decode("example.weather#Forecast", "{\"key\":{\"nil\":null}}")) == json("{\"key\":{\"nil\":null}}"))
    codec.checkDenseMapNulls()
}
private const val STREAM_CHUNK = 64 * 1024
private const val STREAM_CHUNKS = 8192 // 512 MiB, twice the consumer's -Xmx256m heap.
private fun streamChunk(index: Int) = ByteArray(STREAM_CHUNK) { (it + index).toByte() }
private fun streamDigest(): String {
    val digest = MessageDigest.getInstance("SHA-256")
    repeat(STREAM_CHUNKS) { digest.update(streamChunk(it)) }
    return digest.digest().joinToString("") { "%02x".format(it.toInt() and 255) }
}
private fun CoroutineScope.streamSource() = writer { repeat(STREAM_CHUNKS) { channel.writeFully(streamChunk(it), 0, STREAM_CHUNK) } }.channel
/** Real CIO transfers larger than the heap prove that streamed bodies are never buffered. */
private suspend fun streaming() = coroutineScope {
    val total = STREAM_CHUNK.toLong() * STREAM_CHUNKS
    val expected = streamDigest()
    Server { exchange ->
        when (exchange.requestURI.path) {
            "/upload" -> {
                val digest = MessageDigest.getInstance("SHA-256"); val buffer = ByteArray(STREAM_CHUNK); var count = 0L
                exchange.requestBody.use { body -> while (true) { val size = body.read(buffer); if (size < 0) break; digest.update(buffer, 0, size); count += size } }
                val hash = digest.digest().joinToString("") { "%02x".format(it.toInt() and 255) }
                check(exchange.requestHeaders.getFirst("x-amz-content-sha256") == hash) { "streamed body digest" }
                exchange.responseHeaders.add("x-received", count.toString()); exchange.responseHeaders.add("x-digest", hash)
                exchange.sendResponseHeaders(200, -1)
            }
            "/download" -> {
                exchange.responseHeaders.add("x-count", STREAM_CHUNKS.toString())
                exchange.sendResponseHeaders(200, total)
                exchange.responseBody.use { body -> repeat(STREAM_CHUNKS) { body.write(streamChunk(it)) } }
            }
            "/transfer" -> {
                val body = exchange.requestBody.readBytes()
                val crc = java.util.zip.CRC32().apply { update(body) }.value
                check(exchange.requestHeaders.getFirst("x-amz-checksum-crc32") == java.util.Base64.getEncoder().encodeToString(java.nio.ByteBuffer.allocate(4).putInt(crc.toInt()).array()))
                exchange.responseHeaders.add("x-amz-checksum-crc32", exchange.requestHeaders.getFirst("x-amz-checksum-crc32"))
                response(exchange, 200, if (body.decodeToString() == "corrupt") "corrupted".toByteArray() else body)
            }
            else -> response(exchange, 404, "{}")
        }
    }.use { server ->
        val validated = mutableListOf<String>()
        WeatherClient(server.config().copy(timeout = 120.seconds, checksumValidated = { operation, algorithm -> validated.add("$operation:$algorithm") })).use { client ->
            val output = client.upload { body = ByteStream.replay(total, expected) { streamSource() } }
            check(output.received == total && output.digest == expected) { "replayable upload" }
            val chunked = client.upload { body = ByteStream.once(streamSource(), sha256 = expected) }
            check(chunked.received == total && chunked.digest == expected) { "unsized one-shot upload" }
            val unsigned = ByteStream.once(streamSource())
            try { client.upload { body = unsigned }; error("unsigned stream digest accepted") }
            catch (error: IllegalArgumentException) { check(error.message!!.contains("SHA-256")) }
            finally { unsigned.close() } // The caller owns an unsent body; closing cancels its producer.
            val received = client.download { result ->
                check(result.count == STREAM_CHUNKS && result.body.contentLength == total)
                val channel = result.body.openChannel(); val digest = MessageDigest.getInstance("SHA-256"); val buffer = ByteArray(STREAM_CHUNK); var count = 0L
                while (true) { val size = channel.readAvailable(buffer, 0, buffer.size); if (size < 0) break; digest.update(buffer, 0, size); count += size }
                check(digest.digest().joinToString("") { "%02x".format(it.toInt() and 255) } == expected)
                count
            }
            check(received == total) { "download length" }
            val echoed = client.transfer({ body = ByteStream.fromBytes("streamed checksum".toByteArray()); algorithm = TransferAlgorithm.CRC32 }) { it.body.toByteArray(1024).decodeToString() }
            check(echoed == "streamed checksum" && validated == listOf("Transfer:CRC32")) { "streamed checksum validation" }
            try { client.transfer({ body = ByteStream.fromBytes("corrupt".toByteArray()); algorithm = TransferAlgorithm.CRC32 }) { it.body.toByteArray(1024) }; error("corrupt body accepted") }
            catch (_: ChecksumMismatch) { }
            check(validated.size == 1)
        }
    }
}
fun main(args: Array<String>) = runBlocking {
    hardening()
    withTimeout(300000) { streaming() }
    withTimeout(60000) { retries(args[0]); serialization(args[1]); httpBehavior(); failures(); deadlines(); payloads(); weather() }
    println("Kotlin native package, conformance, and weather HTTP passed")
}
