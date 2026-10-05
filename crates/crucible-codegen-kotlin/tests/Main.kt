package example.weather

import example.weather.model.ValidationError
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
import com.google.gson.JsonElement
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpServer
import java.io.Closeable
import java.io.IOException
import java.lang.reflect.InvocationTargetException
import java.net.InetSocketAddress
import java.nio.file.Files
import java.nio.file.Paths
import java.security.MessageDigest
import java.time.Duration
import java.time.Instant
import java.util.concurrent.Executors
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

private fun json(value: String): JsonElement = JsonParser.parseString(value)
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
            val body = JsonObject().also { if (!test.get("error").isJsonNull) it.add("__type", test.get("error")) }
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
    private val instance = type.getConstructor(JsonObject::class.java).newInstance(schema)
    private fun invoke(name: String, vararg args: Any?): Any? {
        val method = type.methods.first { it.name == name && it.parameterCount == args.size }
        try { return method.invoke(instance, *args) } catch (error: InvocationTargetException) { throw error.cause!! }
    }
    fun decode(target: String, wire: String): Any? = invoke("decode", target, json(wire), null)
    fun encode(target: String, value: Any?, format: String? = null): JsonElement = invoke("encode", target, value, format) as JsonElement
    fun header(wire: String): JsonElement = invoke("header", json("""{"target":"example.weather#Tags"}""").asJsonObject, wire) as JsonElement
    fun bound(values: JsonElement): String = invoke("bound", values) as String
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
            check(actual.epochSecond.toString() == instant.get("seconds").asString && actual.nano == instant.get("nanoseconds").asInt) { "$id instant" }
        }
    }
    check(codec.decode("example.weather#Sky", "\"future\"") == Sky("future"))
    val future = """{"future":{"n":9223372036854775807}}"""
    val union = codec.decode("example.weather#Location", future)
    check(union is Location.UnknownVariant && codec.encode("example.weather#Location", union) == json(future))
    check(codec.encode("example.weather#Location", Location.Unknown(kotlin.Unit)) == json("""{"unknown":{}}"""))
    val city = codec.decode("example.weather#CitySummary", """{"id":"c","name":"x","createdAt":-0.000000001,"population":9223372036854775807}""") as CitySummary
    check(city.population == Long.MAX_VALUE && city.createdAt == Instant.ofEpochSecond(-1, 999999999))
    check((codec.decode("example.weather#CitySummary", """{"id":"c","name":"x"}""") as CitySummary).population == 0L)
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
    val nullDocument = codec.decode("example.weather#Forecast", """{"future":null}""") as Forecast
    check(nullDocument[Sky("future")]!!.isJsonNull)
    val binary = byteArrayOf(0, 1, -1, -128)
    check(codec.encode("example.weather#Photo", binary).asString == "AAH/gA==")
    check((codec.decode("example.weather#Photo", "\"AAH/gA==\"") as ByteArray).contentEquals(binary))
    try { codec.decode("example.weather#Tags", "[null]"); error("Null element accepted in a non-sparse list") }
    catch (_: IllegalArgumentException) { }
    for (test in json(String(Files.readAllBytes(Paths.get(path)))).asJsonArray) {
        check(codec.header(test.asJsonObject.get("wire").asString) == test.asJsonObject.get("values"))
        val encoded = codec.bound(test.asJsonObject.get("values"))
        check(encoded == test.asJsonObject.get("encoded").asString)
        check(codec.header(encoded) == test.asJsonObject.get("values"))
    }
    check(codec.encode("example.weather#DateTime", Instant.parse("2026-10-01T00:00:00Z"), "http-date").asString == "Thu, 01 Oct 2026 00:00:00 GMT")
}
private fun exactJsonEquals(a: JsonElement, b: JsonElement): Boolean {
    if (a.isJsonPrimitive && a.asJsonPrimitive.isNumber) return b.isJsonPrimitive && b.asJsonPrimitive.isNumber && a.asBigDecimal.compareTo(b.asBigDecimal) == 0
    if (a.isJsonObject) return b.isJsonObject && a.asJsonObject.keySet() == b.asJsonObject.keySet() && a.asJsonObject.entrySet().all { (key, value) -> exactJsonEquals(value, b.asJsonObject.get(key)) }
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
    val count = AtomicInteger()
    Server { exchange -> count.incrementAndGet(); response(exchange, 200, "invalid") }.use { server ->
        WeatherClient(server.config()).use { client ->
            try { client.getCity(GetCityInput { this.cityId = "x" }); error("invalid JSON accepted") }
            catch (_: com.google.gson.JsonParseException) { check(count.get() == 1) }
        }
    }
    count.set(0)
    Server { exchange ->
        val body = """{"city":{"id":"c","name":"x"}}""".toByteArray()
        exchange.sendResponseHeaders(200, body.size.toLong() + if (count.incrementAndGet() == 1) 10 else 0)
        exchange.responseBody.write(body)
    }.use { server -> WeatherClient(server.config()).use { it.getCity(GetCityInput { this.cityId = "x" }); check(count.get() == 2) } }
    Server { exchange ->
        exchange.sendResponseHeaders(200, 100)
        exchange.responseBody.write('{'.code); exchange.responseBody.flush()
        Thread.sleep(5000)
    }.use { server ->
        WeatherClient(server.config()).use { withTimeout(1500) { it.ping() } }
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
        WeatherClient(server.config().copy(timeout = Duration.ofSeconds(15), maxAttempts = 1)).use { client ->
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
        WeatherClient(server.config().copy(timeout = Duration.ofMillis(250))).use { client ->
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
    val injected = okhttp3.OkHttpClient.Builder().readTimeout(100, TimeUnit.MILLISECONDS).build()
    try {
        Server { exchange ->
            exchange.sendResponseHeaders(200, 100)
            exchange.responseBody.write('{'.code); exchange.responseBody.flush()
            Thread.sleep(5000)
        }.use { server ->
            WeatherClient(server.config().copy(httpClient = injected, maxAttempts = 1)).use { client ->
                try { withTimeout(2000) { client.getCity(GetCityInput { this.cityId = "x" }) }; error("injected timeout ignored") }
                catch (error: NetworkError) { check(error.cause is java.net.SocketTimeoutException) }
            }
            check(!injected.dispatcher.executorService.isShutdown) { "caller-owned transport was closed" }
        }
    } finally {
        injected.dispatcher.executorService.shutdown()
        injected.connectionPool.evictAll()
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
fun main(args: Array<String>) = runBlocking {
    withTimeout(60000) { retries(args[0]); serialization(args[1]); httpBehavior(); failures(); deadlines(); payloads(); weather() }
    println("Kotlin native package, conformance, and weather HTTP passed")
}
