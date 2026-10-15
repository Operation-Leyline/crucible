package example.consumer

import example.weather.ClientConfig
import example.weather.WeatherClient
import example.weather.model.CreateCityInput
import example.weather.model.Location
import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.Headers
import io.ktor.http.HttpStatusCode
import io.ktor.http.content.OutgoingContent
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds
import kotlin.time.Instant
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

// Ktor engines use real dispatchers; run SDK deadlines on the same clock.
internal fun realTest(block: suspend CoroutineScope.() -> Unit) = runTest { withContext(Dispatchers.Default, block) }

/** This module can access only the published SDK's public API. Runs unchanged on JVM, JS and Native. */
class ClientTest {
    @Test fun deadlinesSurviveUntypedCancellationAndActiveCancellationPropagates() = realTest {
        var requests=0
        val http=HttpClient(MockEngine { requests++; respond("{}",HttpStatusCode.OK) })
        var entered=CompletableDeferred<Unit>()
        // Like Ktor's call job, a dependency may discard the timeout type and cause.
        val losingCause: suspend (example.weather.Request) -> Unit = {
            entered.complete(Unit)
            try { CompletableDeferred<Unit>().await() }
            catch (error: kotlinx.coroutines.CancellationException) {
                throw kotlinx.coroutines.CancellationException(error.message)
            }
        }
        val config=ClientConfig(endpoint="https://example.test",httpClient=http,middleware=listOf(losingCause))
        try {
            WeatherClient(config.copy(timeout=20.milliseconds)).use { client ->
                assertFailsWith<kotlinx.coroutines.TimeoutCancellationException> { client.ping() }
            }
            WeatherClient(config).use { client ->
                entered=CompletableDeferred()
                val task=async { client.ping() }
                entered.await(); task.cancelAndJoin()
                assertTrue(task.isCancelled)
            }
            val providerCancellation=kotlinx.coroutines.CancellationException("provider cancelled")
            WeatherClient(config.copy(token={ throw providerCancellation },middleware=emptyList())).use { client ->
                val error=assertFailsWith<kotlinx.coroutines.CancellationException> { client.getCity { cityId="cancelled" } }
                assertEquals(providerCancellation.message,error.message)
                assertTrue(generateSequence<Throwable>(error) { it.cause }.any { it === providerCancellation })
            }
            assertEquals(0,requests)
        } finally { http.close() }
    }
    @Test fun bearerAuthIsPerAttemptAndAnonymousStaysAnonymous() = realTest {
        val sent = mutableListOf<io.ktor.client.request.HttpRequestData>()
        val http = HttpClient(MockEngine { request -> sent.add(request); respond("""{"city":{"id":"c","name":"x"}}""", HttpStatusCode.OK) })
        var calls = 0
        WeatherClient(ClientConfig(endpoint="https://example.test", httpClient=http, token={ "t${++calls}" })).use { client ->
            client.getCity { cityId="c" }; assertEquals("Bearer t1", sent.last().headers["Authorization"])
            client.getCity { cityId="c" }; assertEquals("Bearer t2", sent.last().headers["Authorization"])
            client.anonymousWrite(); assertEquals(null, sent.last().headers["Authorization"])
            assertEquals(2, calls)
        }
        WeatherClient(ClientConfig(endpoint="http://example.test", httpClient=http, token={ "secret" })).use { client ->
            assertFailsWith<IllegalArgumentException> { client.getCity { cityId="c" } }
        }
        WeatherClient(ClientConfig(endpoint="https://example.test", httpClient=http)).use { client ->
            client.getCity { cityId="c" }; assertEquals(null, sent.last().headers["Authorization"])
        }
        http.close()
    }
    @Test fun textAndStructuredPayloadsAndMediaHeaders() = realTest {
        val text = "quoted \"é\"\n"
        val metadata = listOf("{\"x\":1}", "é, two")
        val encoded = metadata.joinToString(", ") { kotlin.io.encoding.Base64.encode(it.encodeToByteArray()) }
        val http = HttpClient(MockEngine { request ->
            val bytes = (request.body as OutgoingContent.ByteArrayContent).bytes()
            if (request.url.encodedPath == "/text") {
                assertTrue(request.headers["Content-Type"]!!.startsWith("text/plain"))
                if (bytes.decodeToString() == "fail") respond("invalid text", HttpStatusCode.UnprocessableEntity)
                else {
                    assertEquals(encoded, request.headers["x-meta"])
                    respond(bytes, HttpStatusCode.OK, Headers.build { append("x-meta", encoded) })
                }
            } else {
                assertEquals("{\"name\":\"record\"}", bytes.decodeToString())
                respond(bytes, HttpStatusCode.OK)
            }
        })
        WeatherClient(ClientConfig(endpoint="https://weather.example", httpClient=http)).use { client ->
            val result = client.text { this.text=text; meta=metadata }
            assertEquals(text, result.text); assertEquals(metadata, result.meta)
            assertEquals("", client.text { this.text=""; meta=metadata }.text)
            val record = client.document { document=example.weather.model.Record { name="record" } }
            assertEquals("record", record.document?.name)
            val error = assertFailsWith<example.weather.model.TextError> { client.text { this.text="fail" } }
            assertEquals("invalid text", error.details); assertEquals(422, error.status)
        }
        http.close()
    }
    @Test fun defaultPlatformTransportUsesPublishedEngine() = realTest {
        WeatherClient(ClientConfig(endpoint=WEATHER_ENDPOINT, token={ "local-token" })).use { client ->
            val city = client.getCity { cityId="platform/engine" }.city
            assertEquals("native transport", city.name)
            assertEquals(Long.MAX_VALUE, city.population)
            assertEquals(Instant.fromEpochSeconds(-1, 999999999), city.createdAt)
        }
    }
    @Test fun exactNumbersPresenceAndEscaping() = realTest {
        var count = 0
        val http = HttpClient(MockEngine { request ->
            count++
            if (request.method.value == "GET") {
                assertEquals("/cities/a%2Fb%20%C3%A9", request.url.encodedPath)
            } else {
                val bytes = (request.body as OutgoingContent.ByteArrayContent).bytes()
                assertEquals("{\"name\":\"kmp\",\"location\":null,\"clientToken\":\"stable\"}", bytes.decodeToString())
            }
            respond("""{"city":{"id":"c","name":"portable","population":9223372036854775807,"createdAt":-0.000000001}}""", HttpStatusCode.OK)
        })
        WeatherClient(ClientConfig(endpoint="https://weather.example", httpClient=http)).use { client ->
            val city = client.getCity { cityId = "a/b é" }.city
            assertEquals(Long.MAX_VALUE, city.population)
            assertEquals(Instant.fromEpochSeconds(-1, 999999999), city.createdAt)
            val input = CreateCityInput { name = "kmp"; clientToken = "stable"; location = null }
            assertTrue(input.hasLocation()); assertEquals(null, input.location)
            assertFalse(input.copy { unsetLocation() }.hasLocation())
            assertEquals("portable", client.createCity(input).city.name)
            assertFailsWith<IllegalArgumentException> { client.getCity { cityId = ".." } }
        }
        assertEquals(2, count)
        assertTrue(http.coroutineContext[Job]!!.isActive)
        http.close()
    }
    @Test fun pagesAreColdAndCancellationStopsTransport() = realTest {
        var count = 0
        val http = HttpClient(MockEngine { request ->
            count++
            val next = request.url.parameters["nextToken"]
            respond(if (next == null) """{"cities":[],"nextToken":"next"}""" else """{"cities":[]}""", HttpStatusCode.OK)
        })
        WeatherClient(ClientConfig(endpoint="https://weather.example", httpClient=http)).use { client ->
            val pages = client.listCitiesPages { maxResults = 1 }
            assertEquals(0, count)
            assertEquals(1, pages.take(1).toList().size)
            assertEquals(1, count)
            assertEquals(2, pages.toList().size)
            assertEquals(3, count)
        }
        http.close()
        val entered = CompletableDeferred<Unit>(); val cancelled = CompletableDeferred<Unit>()
        val waiting = HttpClient(MockEngine {
            entered.complete(Unit)
            try { CompletableDeferred<Unit>().await(); error("unreachable") }
            finally { cancelled.complete(Unit) }
        })
        WeatherClient(ClientConfig(endpoint="https://weather.example", httpClient=waiting, timeout=30.seconds)).use { client ->
            val call = async { client.getCity { cityId="c" } }
            entered.await(); call.cancelAndJoin(); cancelled.await()
        }
        waiting.close()
    }
    @Test fun arbitraryNumbersOpenEnumsSparseCollectionsAndUniqueLists() = realTest {
        val integer = "12345678901234567890123456789012345678901234567890"
        val decimal = "12345678901234567890.12345678901234567890123456789"
        val defaults = example.weather.model.ExtendedValues {}
        assertEquals(integer, defaults.integer.toString()); assertEquals(decimal, defaults.decimal.toStringExpanded())
        var wire = """{"integer":$integer,"decimal":$decimal,"mode":17,"names":["a","b"],"sparseNames":[null,"x"],"sparseMap":{"null":null,"future":17}}"""
        val http = HttpClient(MockEngine { request ->
            if (request.method.value == "POST") {
                val body = Json.parseToJsonElement((request.body as OutgoingContent.ByteArrayContent).bytes().decodeToString()).jsonObject
                assertEquals(integer, body.getValue("integer").jsonPrimitive.content)
                assertEquals(decimal, com.ionspin.kotlin.bignum.decimal.BigDecimal.parseString(body.getValue("decimal").jsonPrimitive.content).toStringExpanded())
            }
            respond(wire, HttpStatusCode.OK)
        })
        WeatherClient(ClientConfig(endpoint="https://weather.example", httpClient=http)).use { client ->
            val result = client.read()
            assertEquals(integer, result.integer.toString()); assertEquals(decimal, result.decimal.toStringExpanded())
            assertEquals(example.weather.model.Mode(17), result.mode)
            assertEquals(listOf("a", "b"), result.names); assertEquals(listOf(null, "x"), result.sparseNames)
            assertEquals(null, result.sparseMap?.get("null")); assertEquals(example.weather.model.Mode(17), result.sparseMap?.get("future"))
            client.echo(result)
            wire = """{"integers":[18446744073709551616,36893488147419103232],"decimals":[12345678901234567890.1234567891,12345678901234567890.1234567892]}"""
            val precise=client.read(); assertEquals(2,precise.integers?.size); assertEquals(2,precise.decimals?.size)
            wire = """{"decimals":[1,1.0]}"""; assertFailsWith<IllegalArgumentException> { client.read() }
            wire = """{"names":["a","a"]}"""; assertFailsWith<IllegalArgumentException> { client.read() }
            wire = """{"integers":[18446744073709551616,18446744073709551616]}"""; assertFailsWith<IllegalArgumentException> { client.read() }
            wire = """{"integer":1.5}"""; assertFailsWith<IllegalArgumentException> { client.read() }
            wire = """{"mode":2147483648}"""; assertFailsWith<IllegalArgumentException> { client.read() }
        }
        http.close()
    }
    @Test fun arbitraryDecimalsKeepValidCompactJson() = realTest {
        val values = listOf("-1", "-10", "-100", "-0.0", "-0.1", "-12.75",
            "12345678901234567890.12345678901234567890123456789",
            "1e400", "-1e400", "-1e-400", "1e1000000", "-1e1000000", "-1e-1000000")
        var expected = com.ionspin.kotlin.bignum.decimal.BigDecimal.parseString("0")
        var requests = 0
        val http = HttpClient(MockEngine { request ->
            requests++
            val body = (request.body as OutgoingContent.ByteArrayContent).bytes().decodeToString()
            val number = Json.parseToJsonElement(body).jsonObject.getValue("decimal").jsonPrimitive
            assertFalse(number.isString)
            assertTrue(number.content.length <= 64, "large exponents stay compact")
            assertEquals(0, com.ionspin.kotlin.bignum.decimal.BigDecimal.parseString(number.content).compareTo(expected))
            respond(body, HttpStatusCode.OK)
        })
        try {
            WeatherClient(ClientConfig(endpoint="https://weather.example", httpClient=http)).use { client ->
                for (text in values) {
                    expected = com.ionspin.kotlin.bignum.decimal.BigDecimal.parseString(text)
                    val input = example.weather.model.ExtendedValues { decimal = expected }
                    val output = client.echo(input)
                    assertEquals(0, output.decimal.compareTo(expected))
                    assertEquals(input.integer, output.integer)
                }
            }
            assertEquals(values.size, requests)
        } finally {
            http.close()
        }
    }
    @Test fun unitDoesNotParseUnexpectedBody() = realTest {
        val http = HttpClient(MockEngine { request ->
            assertEquals(null, request.headers["Authorization"])
            respond("not-json", HttpStatusCode.OK)
        })
        WeatherClient(ClientConfig(endpoint="https://weather.example", httpClient=http, token={ error("anonymous provider invoked") })).use { it.anonymousWrite() }
        http.close()
    }
}
