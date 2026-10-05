package example.consumer

import example.weather.ClientConfig
import example.weather.WeatherClient
import example.weather.model.CreateCityInput
import example.weather.model.Location
import example.weather.model.Problem
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
    @Test fun endpointRulesFunctionsHostLabelsAndSigning() = realTest {
        val sent=mutableListOf<io.ktor.client.request.HttpRequestData>()
        val http=HttpClient(MockEngine { request -> sent.add(request); respond("""{"wire-status":"READY"}""",HttpStatusCode.OK) })
        val config=ClientConfig(httpClient=http,region="local",credentials={ example.weather.AwsCredentials("key","secret") })
        WeatherClient(config).use { client ->
            assertEquals("READY",client.endpointRead { account="customer" }.statusValue)
            assertEquals("customer.local.example.test",sent.last().url.host)
            assertEquals("local",sent.last().headers["x-region"])
            assertTrue(sent.last().headers["authorization"]!!.contains("/local/example/aws4_request"))
            assertEquals("UNSIGNED-PAYLOAD",sent.last().headers["x-amz-content-sha256"])
            for(value in listOf("a.b","-bad","x/y","x@evil","","x".repeat(64))) assertFailsWith<IllegalArgumentException> { client.endpointRead { account=value } }
            assertEquals(1,sent.size)
        }
        WeatherClient(config.copy(disableHostPrefix=true)).use { client -> client.endpointRead { account="ignored" }; assertEquals("local.example.test",sent.last().url.host) }
        WeatherClient(config.copy(region="missing")).use { client -> assertFailsWith<IllegalArgumentException> { client.endpointRead { account="customer" } } }
        http.close()
    }
    @Test fun modeledWaitersUseCompiledQueriesDeadlinesAndCancellation() = realTest {
        var count=0; var payload="""{"wire-status":"PENDING"}"""
        val http=HttpClient(MockEngine {
            count++
            if (count==1) respond("""{"__type":"Missing"}""",HttpStatusCode.NotFound)
            else respond(if (count==2) payload else """{"wire-status":"READY"}""",HttpStatusCode.OK)
        })
        val options=example.weather.WaiterOptions(timeout=2.seconds,minDelay=kotlin.time.Duration.ZERO,maxDelay=kotlin.time.Duration.ZERO)
        WeatherClient(ClientConfig(endpoint="https://example.test",httpClient=http)).use { client ->
            assertEquals("READY",client.waitUntilReady(options)!!.statusValue); assertEquals(3,count)
        }
        http.close()
        val gone=HttpClient(MockEngine { respond("""{"__type":"Missing"}""",HttpStatusCode.NotFound) })
        WeatherClient(ClientConfig(endpoint="https://example.test",httpClient=gone)).use { client -> assertEquals(null,client.waitUntilDeleted(options)) }
        gone.close()
        payload="""{"items":[{"wire-status":"READY"},{"wire-status":"READY"}],"updatedAt":"2026-01-01T00:00:00Z"}"""
        val queries=HttpClient(MockEngine { respond(payload,HttpStatusCode.OK) })
        WeatherClient(ClientConfig(endpoint="https://example.test",httpClient=queries)).use { client ->
            client.waitUntilAllReady(options); client.waitUntilAnyReady(options); client.waitUntilFilteredReady(options)
            client.waitUntilUpdated(options); client.waitUntilCountsMatch(options) { names=listOf("a","b") }; client.waitUntilFunctions(options)
            payload="""{"wire-status":"FAILED","reasons":["job rejected"]}"""
            val failure=assertFailsWith<example.weather.WaiterError> { client.waitUntilReady(options) }
            assertEquals(1,failure.attempts); assertEquals("[\"job rejected\"]",failure.waiterMessage.toString())
            payload="""{"wire-status":"PENDING"}"""
            assertFailsWith<example.weather.WaiterTimeout> { client.waitUntilReady(example.weather.WaiterOptions(timeout=10.milliseconds)) }
        }
        queries.close()
        val slow=HttpClient(MockEngine { kotlinx.coroutines.delay(1000); respond("{}",HttpStatusCode.OK) })
        WeatherClient(ClientConfig(endpoint="https://example.test",httpClient=slow,timeout=1.milliseconds)).use { client ->
            repeat(50) {
                assertFailsWith<kotlinx.coroutines.TimeoutCancellationException> { client.poll() }
            }
            val failure=assertFailsWith<example.weather.WaiterError> { client.waitUntilReady(options) }
            assertTrue(failure.lastError is kotlinx.coroutines.TimeoutCancellationException)
        }
        slow.close()
        val entered=CompletableDeferred<Unit>(); val cancelled=CompletableDeferred<Unit>()
        val waiting=HttpClient(MockEngine { entered.complete(Unit); try { CompletableDeferred<Unit>().await(); error("unreachable") } finally { cancelled.complete(Unit) } })
        WeatherClient(ClientConfig(endpoint="https://example.test",httpClient=waiting)).use { client ->
            val task=async { client.waitUntilReady(options) }; entered.await(); task.cancelAndJoin(); cancelled.await()
        }
        waiting.close()
    }
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
                assertFailsWith<kotlinx.coroutines.TimeoutCancellationException> { client.poll() }
                val failure=assertFailsWith<example.weather.WaiterError> {
                    client.waitUntilReady(example.weather.WaiterOptions(timeout=2.seconds))
                }
                assertTrue(failure.lastError is kotlinx.coroutines.TimeoutCancellationException)
            }
            WeatherClient(config).use { client ->
                assertFailsWith<example.weather.WaiterTimeout> {
                    client.waitUntilReady(example.weather.WaiterOptions(timeout=20.milliseconds))
                }
                entered=CompletableDeferred()
                val task=async { client.poll() }
                entered.await(); task.cancelAndJoin()
                assertTrue(task.isCancelled)
            }
            val providerCancellation=kotlinx.coroutines.CancellationException("provider cancelled")
            WeatherClient(config.copy(token={ throw providerCancellation },middleware=emptyList())).use { client ->
                val error=assertFailsWith<kotlinx.coroutines.CancellationException> { client.getCity { cityId="cancelled" } }
                assertTrue(error === providerCancellation)
            }
            assertEquals(0,requests)
        } finally { http.close() }
    }
    @Test fun waiterLeavesTimeForItsFinalPoll() = realTest {
        var requests=0
        val http=HttpClient(MockEngine {
            requests++
            if (requests>1) kotlinx.coroutines.delay(100)
            respond(if (requests==1) """{"wire-status":"PENDING"}""" else """{"wire-status":"READY"}""",HttpStatusCode.OK)
        })
        try {
            WeatherClient(ClientConfig(endpoint="https://example.test",httpClient=http)).use { client ->
                val options=example.weather.WaiterOptions(timeout=1500.milliseconds,minDelay=1.seconds,maxDelay=1.seconds)
                assertEquals("READY",client.waitUntilReady(options)!!.statusValue)
                assertEquals(2,requests)
                requests=0
                assertFailsWith<example.weather.WaiterTimeout> { client.waitUntilReady(options.copy(timeout=500.milliseconds)) }
                assertEquals(1,requests)
            }
        } finally { http.close() }
    }
    @Test fun authSelectionCachingAndPublishedAwsSignatureVector() = realTest {
        val sent = mutableListOf<io.ktor.client.request.HttpRequestData>()
        val http = HttpClient(MockEngine { request -> sent.add(request); respond("", HttpStatusCode.NoContent) })
        val basic = example.weather.BasicCredentials("user", "secret")
        val config = ClientConfig(endpoint="https://example.test", httpClient=http, basicAuth={ basic }, apiKey={ error("Unrelated provider invoked") })
        WeatherClient(config).use { client ->
            client.authRead(); assertEquals("Basic dXNlcjpzZWNyZXQ=", sent.last().headers["Authorization"])
            client.anonymousWrite(); assertEquals(null, sent.last().headers["Authorization"])
        }
        WeatherClient(config.copy(basicAuth=null, apiKey=null)).use { client ->
            client.authOptional(); assertFailsWith<IllegalArgumentException> { client.authRead() }
        }
        WeatherClient(config.copy(basicAuth=null, apiKey={ "a+/é" })).use { client ->
            client.authRead(); assertEquals("a+/é", sent.last().url.parameters["api key"])
        }
        WeatherClient(config.copy(endpoint="http://example.test", basicAuth=null, apiKey={ "secret" })).use { client ->
            assertFailsWith<IllegalArgumentException> { client.authRead() }
        }
        val credentials = example.weather.AwsCredentials("AKIAIOSFODNN7EXAMPLE", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY")
        WeatherClient(ClientConfig(endpoint="https://examplebucket.s3.amazonaws.com", httpClient=http,
            credentials={ credentials }, region="us-east-1", clock={ Instant.parse("2013-05-24T00:00:00Z") }, headers=mapOf("Range" to "bytes=0-9"))).use { client ->
            client.signed()
            assertTrue(sent.last().headers["Authorization"]!!.endsWith("Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"))
        }
        assertFalse(credentials.toString().contains(credentials.secretAccessKey)); assertFalse(basic.toString().contains("secret"))
        var count=0
        val cached=example.weather.CachedCredentials({ count++; kotlinx.coroutines.delay(5); credentials })
        val values=(1..30).map { async { cached.get() } }.map { it.await() }
        assertEquals(1,count); assertTrue(values.all { it === credentials })
        assertEquals(credentials, example.weather.credentialChain({ null }, cached::get)())
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
                    assertEquals(kotlin.io.encoding.Base64.encode("prefix é".encodeToByteArray()),request.headers["x-extra-value"])
                    respond(bytes, HttpStatusCode.OK, Headers.build { append("x-meta", encoded); append("x-extra-value",request.headers["x-extra-value"]!!) })
                }
            } else {
                assertEquals("{\"name\":\"record\"}", bytes.decodeToString())
                respond(bytes, HttpStatusCode.OK)
            }
        })
        WeatherClient(ClientConfig(endpoint="https://weather.example", httpClient=http)).use { client ->
            val result = client.text { this.text=text; meta=metadata; extras=mapOf("value" to "prefix é") }
            assertEquals(text, result.text); assertEquals(metadata, result.meta); assertEquals("prefix é",result.extras?.get("value"))
            assertEquals("", client.text { this.text=""; meta=metadata; extras=mapOf("value" to "prefix é") }.text)
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
    @Test fun exactNumbersPresenceEscapingAndDigest() = realTest {
        var count = 0
        val http = HttpClient(MockEngine { request ->
            count++
            if (request.method.value == "GET") {
                assertEquals("/cities/a%2Fb%20%C3%A9", request.url.encodedPath)
            } else {
                val bytes = (request.body as OutgoingContent.ByteArrayContent).bytes()
                assertEquals("{\"name\":\"kmp\",\"location\":null,\"clientToken\":\"stable\"}", bytes.decodeToString())
                // Independent hashlib SHA-256 oracle for the exact literal body above.
                assertEquals("493833fc53e0678ce40dda6ef02c858c15a9ba515adcc2036c5ac3d5400a2a31", request.headers["x-amz-content-sha256"])
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
            respond(if (next == null) """{"cities":[],"wire-cursor":"next"}""" else """{"cities":[]}""", HttpStatusCode.OK)
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
    @Test fun dynamicBindingsUseModelNamesAndWireNamesInTheirOwnPlaces() = realTest {
        var count = 0
        val http = HttpClient(MockEngine { request ->
            count++
            assertEquals("/files/folder/a%20b", request.url.encodedPath)
            assertEquals(listOf("hello world", "/"), request.url.parameters.getAll("tag"))
            assertEquals("", request.url.parameters["empty"])
            assertEquals("abc", request.headers["x-meta-trace"])
            assertEquals("{\"wire-value\":\"hello\"}", (request.body as OutgoingContent.ByteArrayContent).bytes().decodeToString())
            if (count == 1) respond("""{"wire-value":"result"}""", HttpStatusCode.PartialContent, Headers.build { append("x-meta-trace", "abc") })
            else respond("""{"__type":"Problem"}""", HttpStatusCode.UnprocessableEntity, Headers.build { append("x-meta-reason", "invalid") })
        })
        WeatherClient(ClientConfig(endpoint="https://weather.example", httpClient=http, maxAttempts=1)).use { client ->
            val input = example.weather.model.Input {
                key="folder/a b"; query=mapOf("tag" to listOf("hello world", "/"), "empty" to listOf(""))
                metadata=mapOf("trace" to "abc"); value="hello"
            }
            val result = client.get(input)
            assertEquals(206, result.statusValue); assertEquals("result", result.value); assertEquals("abc", result.metadata?.get("trace"))
            val error = assertFailsWith<Problem> { client.get(input) }
            assertEquals(422, error.status); assertEquals(422, error.statusValue); assertEquals("invalid", error.details?.get("reason"))
            assertFailsWith<IllegalArgumentException> { client.get { key="folder/../other" } }
        }
        assertEquals(2, count); http.close()
    }
    @Test fun arbitraryNumbersOpenEnumsSparseCollectionsAndSets() = realTest {
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
            assertEquals(setOf("a", "b"), result.names); assertEquals(listOf(null, "x"), result.sparseNames)
            assertEquals(null, result.sparseMap?.get("null")); assertEquals(example.weather.model.Mode(17), result.sparseMap?.get("future"))
            client.echo(result)
            wire = """{"integers":[18446744073709551616,36893488147419103232],"decimals":[12345678901234567890.1234567891,12345678901234567890.1234567892]}"""
            val precise=client.read(); assertEquals(2,precise.integers?.size); assertEquals(2,precise.decimals?.size)
            wire = """{"decimals":[1,1.0]}"""; assertFailsWith<IllegalArgumentException> { client.read() }
            wire = """{"integers":[18446744073709551616,18446744073709551616]}"""; assertFailsWith<IllegalArgumentException> { client.read() }
            wire = """{"names":["a","a"]}"""; assertFailsWith<IllegalArgumentException> { client.read() }
            wire = """{"integer":1.5}"""; assertFailsWith<IllegalArgumentException> { client.read() }
            wire = """{"mode":2147483648}"""; assertFailsWith<IllegalArgumentException> { client.read() }
        }
        http.close()
    }
    @Test fun unitDoesNotParseUnexpectedBodyAndAnonymousStaysAnonymous() = realTest {
        val http = HttpClient(MockEngine { request ->
            assertEquals(null, request.headers["Authorization"])
            assertEquals("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", request.headers["x-amz-content-sha256"])
            respond("not-json", HttpStatusCode.OK)
        })
        WeatherClient(ClientConfig(endpoint="https://weather.example", httpClient=http, token={ error("anonymous provider invoked") })).use { it.anonymousWrite() }
        http.close()
    }
}
