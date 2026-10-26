package example.error_docs

import com.sun.net.httpserver.HttpServer
import java.net.InetSocketAddress
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.runBlocking

private data class Case(val status: Int, val identity: String?, val body: String, val header: String?, val page: String?, val typed: Boolean = true)

fun main() = runBlocking {
    fun denied(reason: String, suffix: String) = Case(403, "Denied", "{\"message\":\"denied\",\"detail\":$reason}", null, "Denied$suffix")
    val cases = listOf(
        Case(419, "DefaultReason", "{\"message\":\"denied\"}", null, "DefaultReason/reason-access%2Fneeded", true),
        Case(420, "DefaultEmptyReason", "{\"message\":\"denied\"}", null, "DefaultEmptyReason/reason-", true),
        Case(419, "DefaultReason", "{\"message\":\"denied\",\"detail\":null}", null, "DefaultReason", false),
        Case(419, "DefaultReason", "{\"message\":\"denied\",\"detail\":\"future\"}", null, "DefaultReason", true),
        denied("\"access/needed\"", "/reason-access%2Fneeded"),
        denied("\"\"", "/reason-"), denied("\"..\"", "/reason-%2E%2E"),
        denied("\"caf\u00e9\"", "/reason-caf%C3%A9"), denied("null", ""),
        denied("\"future\"", ""), denied("\"secret\"", ""),
        Case(403, "Denied", "{\"message\":\"denied\"}", null, "Denied"),
        Case(418, "MappedReasonError", "{\"message\":\"denied\",\"failureReason\":\"access/needed\"}", null, "MappedReasonError/reason-access%2Fneeded"),
        Case(409, "HeaderError", "{\"message\":\"denied\",\"reason\":\"future\"}", "access/needed", "HeaderError/reason-access%2Fneeded"),
        Case(409, "HeaderError", "{\"message\":\"denied\"}", null, "HeaderError"),
        Case(409, "HeaderError", "{\"message\":\"denied\"}", "", "HeaderError/reason-"),
        Case(422, "PayloadError", "access/needed", null, "PayloadError/reason-access%2Fneeded"),
        Case(422, "PayloadError", "", null, "PayloadError/reason-"),
        Case(422, "PayloadError", "future", null, "PayloadError"),
        Case(400, "PlainError", "{\"message\":\"denied\",\"reason\":\"access/needed\"}", null, "PlainError"),
        Case(402, "RequiredReason", "{\"reason\":\"access/needed\"}", null, "RequiredReason/reason-access%2Fneeded"),
        Case(402, "RequiredReason", "{}", null, "RequiredReason", false),
        Case(402, "RequiredReason", "{\"reason\":null}", null, "RequiredReason", false),
        Case(403, "Denied", "{\"detail\":\"access/needed\"}", null, "Denied", false),
        Case(403, "Denied", "{\"message\":\"denied\",\"detail\":{}}", null, "Denied", false),
        Case(403, "UnknownFuture", "{\"message\":\"denied\",\"detail\":\"access/needed\"}", null, null, false),
        Case(403, null, "{\"message\":\"denied\",\"detail\":\"access/needed\"}", null, "Denied/reason-access%2Fneeded"),
        Case(599, null, "{}", null, null, false),
        Case(403, "Denied", "{\"message\":\"denied\",\"docUrl\":\"https://evil.test/\"}", null, "Denied"),
    )
    val selected = AtomicReference<Case>()
    val failure = AtomicReference<Throwable>()
    val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
    server.createContext("/inspect") { exchange ->
        try {
            check(exchange.requestMethod == "GET")
            val case = selected.get()
            case.identity?.let { exchange.responseHeaders.add("x-crucible-error", it) }
            case.header?.let { exchange.responseHeaders.add("X-Error-Reason", it) }
            exchange.responseHeaders.add("x-request-id", "documentation-consumer")
            val bytes = case.body.toByteArray(Charsets.UTF_8)
            exchange.sendResponseHeaders(case.status, bytes.size.toLong())
            exchange.responseBody.write(bytes)
        } catch (error: Throwable) { failure.set(error) }
        finally { exchange.close() }
    }
    server.start()
    val client = TroubleshootingClient(ClientConfig(endpoint = "http://127.0.0.1:${server.address.port}", maxAttempts = 1))
    try {
        for (case in cases) {
            selected.set(case)
            val actual = try { client.inspect(); error("expected service error") } catch (error: ServiceError) { error }
            val expected = case.page?.let { "https://docs.example.test/errors/$it" }
            check(actual.docUrl == expected) { "$case URL=${actual.docUrl}" }
            check((actual::class != TroubleshootingException::class) == case.typed) { "$case type=${actual::class}" }
            check(actual.status == case.status && actual.requestId == "documentation-consumer")
        }
        failure.get()?.let { throw AssertionError(it) }
    } finally { client.close(); server.stop(0) }
    println("Kotlin error documentation: ${cases.size} real HTTP cases passed")
}
