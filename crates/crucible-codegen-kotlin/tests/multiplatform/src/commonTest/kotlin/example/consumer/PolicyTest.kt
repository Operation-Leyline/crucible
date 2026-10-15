package example.consumer

import example.policy.AdaptiveRateLimiter
import example.policy.AttemptFinished
import example.policy.AttemptStarted
import example.policy.ClientConfig
import example.policy.CubicCalculator
import example.policy.NetworkError
import example.policy.OperationFinished
import example.policy.OperationStarted
import example.policy.RecordsClient
import example.policy.RetryMode
import example.policy.RetryQuota
import example.policy.RuntimeClock
import example.policy.ServiceError
import example.policy.TelemetryHook
import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.MockRequestHandleScope
import io.ktor.client.engine.mock.respond
import io.ktor.client.network.sockets.ConnectTimeoutException
import io.ktor.client.request.HttpResponseData
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertTrue
import kotlin.time.Duration
import kotlin.time.Duration.Companion.seconds
import kotlin.time.DurationUnit
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.yield
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.double
import kotlinx.serialization.json.int
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/** The shared runtime-policy vectors (tests/conformance), against a client generated from runtime-policy.json. */
private val QUOTA = Json.parseToJsonElement(RETRY_QUOTA_VECTORS).jsonObject
private val ADAPTIVE = Json.parseToJsonElement(ADAPTIVE_RETRY_VECTORS).jsonObject
private val TELEMETRY = Json.parseToJsonElement(TELEMETRY_VECTORS).jsonObject
private const val ENDPOINT = "https://records.example"
private val JSON_HEADERS = headersOf("content-type", "application/json")
private val RESPONSES = mapOf("ok" to (200 to "{}"), "unavailable" to (503 to """{"__type":"Unavailable"}"""),
    "throttled" to (429 to """{"__type":"Throttled"}"""), "denied" to (400 to """{"__type":"Denied"}"""))

private fun JsonElement.obj(name: String) = jsonObject.getValue(name).jsonObject
private fun JsonElement.arr(name: String) = jsonObject.getValue(name).jsonArray
private fun JsonElement.str(name: String) = jsonObject[name]?.takeUnless { it == JsonNull }?.jsonPrimitive?.content
private fun JsonElement.num(name: String) = jsonObject.getValue(name).jsonPrimitive.double
private fun JsonElement.flag(name: String) = jsonObject.getValue(name).jsonPrimitive.boolean
private fun Duration.seconds(): Double = toDouble(DurationUnit.SECONDS)

/** Manual time: sleeps advance it instantly and are recorded. */
class FakeClock : RuntimeClock {
    var time: Duration = Duration.ZERO
    val sleeps = mutableListOf<Duration>()
    override fun now(): Duration = time
    override suspend fun sleep(duration: Duration) { sleeps += duration; time += duration; yield() }
}

private class Recorder : TelemetryHook {
    val events = mutableListOf<Any>()
    override fun operationStarted(event: OperationStarted) { events += event }
    override fun attemptStarted(event: AttemptStarted) { events += event }
    override fun attemptFinished(event: AttemptFinished) { events += event }
    override fun operationFinished(event: OperationFinished) { events += event }
}

private fun client(http: HttpClient, clock: RuntimeClock = RuntimeClock.System, mode: RetryMode = RetryMode.LEGACY,
    attempts: Int = 3, telemetry: TelemetryHook? = null, key: String = "key", baseDelay: Duration = Duration.ZERO) =
    RecordsClient(ClientConfig(endpoint = ENDPOINT, httpClient = http, token = { key }, baseDelay = baseDelay, retryMode = mode,
        maxAttempts = attempts, telemetry = telemetry, runtimeClock = clock))

private fun MockRequestHandleScope.reply(spec: JsonObject): HttpResponseData {
    if (spec.str("transport") == "timeout") throw ConnectTimeoutException("timed out")
    val headers = (spec["headers"] as? JsonObject).orEmpty().map { (name, value) -> name to listOf(value.jsonPrimitive.content) } + ("content-type" to listOf("application/json"))
    return respond((spec["json"] ?: JsonObject(emptyMap())).toString(), HttpStatusCode.fromValue(spec["status"]!!.jsonPrimitive.int), headersOf(*headers.toTypedArray()))
}

/** Calls an operation by its model name with the vector's JSON input. */
private suspend fun RecordsClient.invoke(call: String, input: JsonElement) {
    when (call) {
        "GetRecord" -> getRecord { table = input.str("table"); key = input.str("key") }
        "PutRecord" -> putRecord {
            table = input.str("table")
            input.str("secret")?.let { secret = it }; input.str("ticket")?.let { ticket = it }
            input.str("label")?.let { label = it }; input.str("value")?.let { value = it }
        }
        "ListRecords" -> listRecords()
        else -> error("unknown operation $call")
    }
}

private suspend fun outcome(block: suspend () -> Unit): String = try { block(); "success" }
    catch (error: ServiceError) { error.errorName }

private fun record(event: Any): Map<String, Any?> = when (event) {
    is OperationStarted -> mapOf("event" to "operationStarted", "invocationId" to event.invocationId, "service" to event.service,
        "operation" to event.operation, "time" to event.time.seconds())
    is AttemptStarted -> mapOf("event" to "attemptStarted", "invocationId" to event.invocationId, "service" to event.service,
        "operation" to event.operation, "attempt" to event.attempt, "method" to event.method, "endpoint" to event.endpoint,
        "target" to event.target, "headers" to event.headers,
        "requestBytes" to event.requestBytes, "time" to event.time.seconds())
    is AttemptFinished -> mapOf("event" to "attemptFinished", "invocationId" to event.invocationId, "service" to event.service,
        "operation" to event.operation, "attempt" to event.attempt, "status" to event.status, "error" to event.error,
        "requestId" to event.requestId, "responseBytes" to event.responseBytes, "latency" to event.latency.seconds(),
        "retryReason" to event.retryReason, "retryDelay" to event.retryDelay?.seconds(), "time" to event.time.seconds())
    is OperationFinished -> mapOf("event" to "operationFinished", "invocationId" to event.invocationId, "service" to event.service,
        "operation" to event.operation, "attempts" to event.attempts, "status" to event.status, "error" to event.error,
        "latency" to event.latency.seconds(), "time" to event.time.seconds())
    else -> error("unknown event $event")
}

/** Only the listed fields are compared; "*" matches any non-null value. */
private fun matches(expected: JsonElement, actual: Any?): Boolean = when {
    expected == JsonNull -> actual == null
    expected is JsonObject -> actual is Map<*, *> && expected.all { (key, value) -> actual.containsKey(key) && matches(value, actual[key]) }
    expected is JsonPrimitive && expected.isString -> if (expected.content == "*") actual != null else actual == expected.content
    expected is JsonPrimitive && expected.booleanOrNull != null -> actual == expected.booleanOrNull
    expected is JsonPrimitive -> actual is Number && actual.toDouble() == expected.double
    else -> false
}

class PolicyTest {
    @Test fun constantsMatchTheVectors() {
        val quota = QUOTA.getValue("constants")
        assertEquals(listOf(quota.num("initialRetryTokens"), quota.num("retryCost"), quota.num("retryTimeoutCost"), quota.num("noRetryIncrement")),
            listOf(RetryQuota.INITIAL_RETRY_TOKENS, RetryQuota.RETRY_COST, RetryQuota.RETRY_TIMEOUT_COST, RetryQuota.NO_RETRY_INCREMENT).map { it.toDouble() })
        val adaptive = ADAPTIVE.getValue("constants")
        assertEquals(listOf("minFillRate", "minCapacity", "smoothing", "timeBucketRange", "beta", "scaleConstant", "maxRateAdjustScale").map { adaptive.num(it) },
            listOf(AdaptiveRateLimiter.MIN_FILL_RATE, AdaptiveRateLimiter.MIN_CAPACITY, AdaptiveRateLimiter.SMOOTHING, AdaptiveRateLimiter.TIME_BUCKET_RANGE,
                AdaptiveRateLimiter.BETA, AdaptiveRateLimiter.SCALE_CONSTANT, AdaptiveRateLimiter.MAX_RATE_ADJUST_SCALE))
    }

    @Test fun quotaSteps() = runTest {
        for (case in QUOTA.arr("quota")) {
            val quota = RetryQuota(case.jsonObject["initial"]?.jsonPrimitive?.int ?: RetryQuota.INITIAL_RETRY_TOKENS)
            for (step in case.arr("steps")) {
                if ("acquire" in step.jsonObject) assertEquals(step.flag("granted"), quota.acquire(step.str("acquire") == "timeout") != null, "${case.str("name")} $step")
                else quota.release(step.jsonObject.getValue("release").jsonPrimitive.intOrNull)
                assertEquals(step.num("available").toInt(), quota.available, "${case.str("name")} $step")
            }
        }
    }

    @Test fun quotaThroughTheClient() = realTest {
        for (case in QUOTA.arr("calls")) {
            val script = ArrayDeque<String>()
            var sent = 0
            val http = HttpClient(MockEngine {
                sent++
                val token = script.removeFirst()
                if (token == "timeout") throw ConnectTimeoutException("timed out")
                val (status, body) = RESPONSES.getValue(token)
                respond(body, HttpStatusCode.fromValue(status), JSON_HEADERS)
            })
            client(http, FakeClock(), RetryMode.valueOf(case.str("mode")!!.uppercase())).use { client ->
                for (step in case.arr("steps")) {
                    repeat(step.jsonObject["repeat"]?.jsonPrimitive?.int ?: 1) {
                        script.clear(); script.addAll(step.arr("responses").map { it.jsonPrimitive.content }); sent = 0
                        val success = try { client.listRecords(); true } catch (_: ServiceError) { false } catch (_: NetworkError) { false }
                        assertEquals(step.num("attempts").toInt(), sent, "${case.str("name")} $step")
                        assertEquals(step.flag("success"), success, "${case.str("name")} $step")
                    }
                    assertEquals(step.jsonObject.getValue("available").jsonPrimitive.intOrNull, client.retryQuota?.available, "${case.str("name")} $step")
                }
            }
            http.close()
        }
    }

    @Test fun cubic() {
        val success = ADAPTIVE.obj("cubicSuccess")
        val calculator = CubicCalculator(success.num("lastMaxRate"), success.num("lastThrottleTime"))
        for (step in success.arr("expect")) assertEquals(step.num("rate"), calculator.success(step.num("time")), 1e-6)
        val throttle = ADAPTIVE.obj("cubicThrottle")
        val cubic = CubicCalculator(throttle.num("lastMaxRate"), throttle.num("lastThrottleTime"))
        var last = 0.0
        for (step in throttle.arr("steps")) {
            val rate = if (step.flag("throttled")) cubic.throttle(last, step.num("time")) else cubic.success(step.num("time"))
            assertEquals(step.num("rate"), rate, 1e-6, step.toString())
            last = rate
        }
    }

    @Test fun rateUpdates() {
        val limiter = AdaptiveRateLimiter(0.0)
        for (step in ADAPTIVE.getValue("rateUpdates").jsonArray) {
            limiter.afterAttempt(step.flag("throttled"), step.num("time"))
            assertEquals(step.num("measuredRate"), limiter.measuredRate, 1e-6, step.toString())
            assertEquals(step.num("fillRate"), limiter.fillRate, 1e-6, step.toString())
            assertEquals(step.flag("enabled"), limiter.enabled, step.toString())
        }
    }

    @Test fun sendDelays() {
        val limiter = AdaptiveRateLimiter(0.0)
        for (step in ADAPTIVE.getValue("sendDelays").jsonArray) {
            if (step.jsonObject["send"]?.jsonPrimitive?.boolean == true) assertEquals(step.num("delay"), limiter.beforeSend(step.num("time")), 1e-6, step.toString())
            else {
                limiter.afterAttempt(step.flag("throttled"), step.num("time"))
                assertEquals(step.num("fillRate"), limiter.fillRate, 1e-6, step.toString())
            }
        }
    }

    @Test fun adaptiveClientWaitsForSendTokensAfterThrottling() = realTest {
        val clock = FakeClock()
        val statuses = ArrayDeque(listOf(200, 429, 200))
        val http = HttpClient(MockEngine {
            clock.time += 0.01.seconds // each response arrives a little later
            val status = statuses.removeFirst()
            respond(if (status == 429) """{"__type":"Throttled"}""" else "{}", HttpStatusCode.fromValue(status), JSON_HEADERS)
        })
        client(http, clock, RetryMode.ADAPTIVE).use { client ->
            client.listRecords()
            assertEquals(emptyList(), clock.sleeps, "no send delay before the first throttle")
            client.listRecords()
        }
        // The retry after the throttle waited for a send token; the backoff itself is zero.
        assertEquals(Duration.ZERO, clock.sleeps[0])
        assertTrue(clock.sleeps[1].isPositive(), clock.sleeps.toString())
        http.close()
    }

    @Test fun standardAndLegacyNeverRateLimit() = realTest {
        for (mode in listOf(RetryMode.LEGACY, RetryMode.STANDARD)) {
            val clock = FakeClock()
            val http = HttpClient(MockEngine { respond("""{"__type":"Throttled"}""", HttpStatusCode.TooManyRequests, JSON_HEADERS) })
            client(http, clock, mode).use { client ->
                assertEquals("Throttled", assertFailsWith<ServiceError> { client.listRecords() }.errorName)
                assertEquals(if (mode == RetryMode.LEGACY) null else 490, client.retryQuota?.available)
            }
            assertEquals(listOf(Duration.ZERO, Duration.ZERO), clock.sleeps)
            http.close()
        }
    }

    @Test fun backoffUsesTheInjectedClockAndRetryAfter() = realTest {
        val clock = FakeClock()
        val replies = ArrayDeque(listOf("2", null, "ok"))
        val http = HttpClient(MockEngine {
            when (val reply = replies.removeFirst()) {
                "ok" -> respond("{}", HttpStatusCode.OK, JSON_HEADERS)
                null -> respond("""{"__type":"Unavailable"}""", HttpStatusCode.ServiceUnavailable, JSON_HEADERS)
                else -> respond("""{"__type":"Unavailable"}""", HttpStatusCode.ServiceUnavailable, headersOf("content-type" to listOf("application/json"), "retry-after" to listOf(reply)))
            }
        })
        client(http, clock, RetryMode.STANDARD).use { it.listRecords() }
        assertEquals(listOf(2.seconds, Duration.ZERO), clock.sleeps)
        http.close()
    }

    @Test fun telemetryScenarios() = realTest {
        for (case in TELEMETRY.arr("scenarios")) {
            val recorder = Recorder()
            val responses = ArrayDeque(case.arr("responses").map { it.jsonObject })
            val http = HttpClient(MockEngine { reply(responses.removeFirst()) })
            client(http, FakeClock(), telemetry = recorder, key = "api-key-secret").use { client ->
                try { client.invoke(case.str("call")!!, case.jsonObject.getValue("input")) } catch (_: Exception) { }
            }
            http.close()
            val name = case.str("name")
            val records = recorder.events.map(::record)
            val expected = case.arr("events")
            assertEquals(expected.size, records.size, "$name: $records")
            expected.zip(records).forEach { (want, got) -> assertTrue(matches(want, got), "$name: $want\n!=\n$got") }
            val text = recorder.events.toString()
            for (secret in case.arr("secrets")) assertTrue(secret.jsonPrimitive.content !in text, "$name leaked ${secret.jsonPrimitive.content}: $text")
            assertEquals(1, records.filter { it["operation"] == case.str("call") }.map { it["invocationId"] }.toSet().size, "$name: one invocation id per call")
            records.zipWithNext().forEach { (first, second) -> assertTrue((first["time"] as Double) <= (second["time"] as Double), name) }
        }
    }

    @Test fun aFailingHookNeverChangesTheOutcome() = realTest {
        val broken = object : TelemetryHook {
            override fun attemptStarted(event: AttemptStarted) { throw IllegalStateException("hook bug") }
            override fun operationFinished(event: OperationFinished) { throw IllegalStateException("hook bug") }
        }
        val http = HttpClient(MockEngine { respond("""{"count":2}""", HttpStatusCode.OK, JSON_HEADERS) })
        client(http, telemetry = broken).use { assertEquals(2, it.listRecords().count) }
        http.close()
    }

    @Test fun latencyAndBytesUseTheInjectedClock() = realTest {
        val clock = FakeClock()
        val recorder = Recorder()
        val http = HttpClient(MockEngine { clock.time += 0.25.seconds; respond("""{"count":7}""", HttpStatusCode.OK, JSON_HEADERS) })
        client(http, clock, telemetry = recorder).use { it.listRecords() }
        http.close()
        val finished = recorder.events.filterIsInstance<AttemptFinished>().single()
        assertEquals(0.25.seconds, finished.latency)
        assertEquals("""{"count":7}""".length.toLong(), finished.responseBytes)
        assertEquals(0.25.seconds, (recorder.events.last() as OperationFinished).latency)
    }

    @Test fun cancellationFinishesTheOperation() = realTest {
        val recorder = Recorder()
        val started = CompletableDeferred<Unit>()
        val http = HttpClient(MockEngine { started.complete(Unit); awaitCancellation() })
        client(http, telemetry = recorder).use { client ->
            coroutineScope {
                val call = async { client.listRecords() }
                started.await()
                call.cancelAndJoin()
            }
        }
        http.close()
        assertEquals(listOf("OperationStarted", "AttemptStarted", "AttemptFinished", "OperationFinished"), recorder.events.map { it::class.simpleName })
        assertNotNull((recorder.events.last() as OperationFinished).error)
    }
}
