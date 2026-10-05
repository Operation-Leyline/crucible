package example.consumer

import example.stream.ByteStream
import example.stream.ChecksumMismatch
import example.stream.ClientConfig
import example.stream.StreamingClient
import example.stream.model.TransferAlgorithm
import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.MockRequestHandleScope
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.client.request.HttpResponseData
import io.ktor.http.Headers
import io.ktor.http.HttpStatusCode
import io.ktor.http.content.OutgoingContent
import io.ktor.utils.io.ByteReadChannel
import io.ktor.utils.io.WriterJob
import io.ktor.utils.io.readAvailable
import io.ktor.utils.io.toByteArray
import io.ktor.utils.io.writeFully
import io.ktor.utils.io.writer
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeout

private const val CHUNK = 64 * 1024
private const val CHUNKS = 512
private const val BOUND = 8L * 1024 * 1024

/** Producers live outside the client's coroutines, so their completion proves the client cancelled them. */
private val producers = CoroutineScope(Dispatchers.Default + Job())

private fun text(value: String) = ByteReadChannel(value.encodeToByteArray())
private fun stalled(): WriterJob = producers.writer { awaitCancellation() }
private fun endless(): WriterJob = producers.writer { val chunk = ByteArray(1024); while (true) channel.writeFully(chunk, 0, chunk.size) }

/** Reads a request body as the transport would, through its WriteChannelContent. */
private suspend fun OutgoingContent.channel(): ByteReadChannel = when (this) {
    is OutgoingContent.WriteChannelContent -> CoroutineScope(currentCoroutineContext()).writer { writeTo(channel) }.channel
    is OutgoingContent.ByteArrayContent -> ByteReadChannel(bytes())
    else -> ByteReadChannel(byteArrayOf())
}
private suspend fun OutgoingContent.read(): ByteArray = channel().toByteArray()
private fun streaming(timeout: kotlin.time.Duration = 30.seconds, validated: ((String, String) -> Unit)? = null,
    handler: suspend MockRequestHandleScope.(HttpRequestData) -> HttpResponseData) =
    StreamingClient(ClientConfig(endpoint = "https://stream.test", httpClient = HttpClient(MockEngine(handler)), baseDelay = 0.milliseconds, timeout = timeout, checksumValidated = validated))

/** Streams run on JVM, JS and Native through the published package's public API. */
class StreamingTest {
    @Test fun oneShotUploadsDispatchOnceAndReplayFactoriesRunPerAttempt() = realTest {
        var attempts = 0
        streaming { request -> attempts++; assertEquals("one shot", request.body.read().decodeToString()); respond("{}", HttpStatusCode.ServiceUnavailable) }.use { client ->
            val body = ByteStream.once(text("one shot"))
            assertFailsWith<example.stream.ServiceError> { client.upload { this.body = body } }
            assertEquals(1, attempts)
            assertFailsWith<IllegalStateException> { client.upload { this.body = body } }
            assertEquals(1, attempts)
        }
        attempts = 0; var factories = 0
        streaming { request ->
            attempts++; assertEquals("same bytes", request.body.read().decodeToString())
            respond("", if (attempts == 1) HttpStatusCode.ServiceUnavailable else HttpStatusCode.OK, Headers.build { append("x-received", "10") })
        }.use { client ->
            val output = client.upload { body = ByteStream.replay { factories++; text("same bytes") } }
            assertEquals(10L, output.received); assertEquals(2, factories); assertEquals(2, attempts)
        }
    }

    @Test fun requiresLengthAndModeledBoundsCheckDeclaredAndActualBytes() = realTest {
        val lengths = mutableListOf<Long?>()
        streaming { request -> lengths.add(request.body.contentLength); request.body.read(); respond("", HttpStatusCode.OK) }.use { client ->
            assertFailsWith<IllegalArgumentException> { client.sized { body = ByteStream.once(text("abc")) } }
            assertFailsWith<IllegalArgumentException> { client.sized { body = ByteStream.once(text("abcd"), contentLength = 4) } }
            assertEquals(0, lengths.size)
            client.sized { body = ByteStream.once(text("abc"), contentLength = 3) }
            assertEquals(listOf<Long?>(3), lengths)
            assertTrue(assertFailsWith<IllegalArgumentException> { client.sized { body = ByteStream.once(text("ab"), contentLength = 3) } }.message!!.contains("ended before"))
            assertTrue(assertFailsWith<IllegalArgumentException> { client.sized { body = ByteStream.replay(3) { text("abcd") } } }.message!!.contains("exceeds"))
            client.upload { body = ByteStream.once(text("unknown length")) }
            assertNull(lengths.last())
        }
    }

    @Test fun largeUploadsAndDownloadsUseBoundedPullThroughBuffers() = realTest {
        var produced = 0L; var consumed = 0L; var widest = 0L
        fun source() = producers.writer { val chunk = ByteArray(CHUNK) { it.toByte() }; repeat(CHUNKS) { produced += CHUNK; channel.writeFully(chunk, 0, CHUNK) } }.channel
        suspend fun drain(channel: ByteReadChannel) {
            val buffer = ByteArray(CHUNK)
            while (true) {
                val size = channel.readAvailable(buffer, 0, buffer.size); if (size < 0) break
                consumed += size; widest = maxOf(widest, produced - consumed)
            }
        }
        streaming { request -> drain(request.body.channel()); respond("", HttpStatusCode.OK, Headers.build { append("x-received", consumed.toString()) }) }.use { client ->
            val total = CHUNK.toLong() * CHUNKS
            assertEquals(total, client.upload { body = ByteStream.replay(total) { source() } }.received)
            assertEquals(total, consumed); assertTrue(widest <= BOUND, "upload buffered $widest bytes")
        }
        produced = 0; consumed = 0; widest = 0
        streaming { respond(source(), HttpStatusCode.OK, Headers.build { append("x-count", "7") }) }.use { client ->
            val count = client.download { output -> assertEquals(0L, consumed); drain(output.body.openChannel()); output.count }
            assertEquals(7, count); assertEquals(CHUNK.toLong() * CHUNKS, consumed); assertTrue(widest <= BOUND, "download buffered $widest bytes")
        }
    }

    @Test fun cancellationAndDeadlinesReleaseStreamSources() = realTest {
        val response = endless(); val reading = CompletableDeferred<Unit>()
        streaming { respond(response.channel, HttpStatusCode.OK) }.use { client ->
            val call = launch { client.download { output -> val channel = output.body.openChannel(); val buffer = ByteArray(1024); while (channel.readAvailable(buffer, 0, buffer.size) >= 0) reading.complete(Unit); error("endless body ended") } }
            reading.await(); call.cancelAndJoin()
            withTimeout(10.seconds) { response.job.join() }
        }
        val upload = endless(); val started = CompletableDeferred<Unit>()
        streaming { request -> val channel = request.body.channel(); channel.readAvailable(ByteArray(1), 0, 1); started.complete(Unit); awaitCancellation() }.use { client ->
            val call = launch { client.upload { body = ByteStream.once(upload.channel) } }
            started.await(); call.cancelAndJoin()
            withTimeout(10.seconds) { upload.job.join() }
        }
        val stalled = stalled()
        streaming(timeout = 300.milliseconds) { respond(stalled.channel, HttpStatusCode.OK) }.use { client ->
            assertFailsWith<TimeoutCancellationException> { client.download { output -> output.body.toByteArray(16) } }
            // A stalled producer never writes, so it observes the cancelled channel only by polling it.
            withTimeout(10.seconds) { while (!stalled.channel.isClosedForRead) delay(10) }
            stalled.job.cancel()
        }
    }

    @Test fun unreadAndRejectedResponsesCancelTheirBodies() = realTest {
        val unread = endless()
        streaming { respond(unread.channel, HttpStatusCode.OK) }.use { client ->
            val body = client.download { output -> output.body }
            withTimeout(10.seconds) { unread.job.join() }
            assertFailsWith<IllegalStateException> { body.openChannel() }
        }
        val invalid = endless()
        streaming { respond(invalid.channel, HttpStatusCode.OK, Headers.build { append("x-count", "invalid") }) }.use { client ->
            assertFailsWith<Exception> { client.download { error("consumer must not run") } }
            withTimeout(10.seconds) { invalid.job.join() }
        }
        streaming { respond("abc", HttpStatusCode.OK, Headers.build { append("content-length", "3") }) }.use { client ->
            assertEquals("abc", client.download { it.body.toByteArray(3).decodeToString() })
            assertFailsWith<IllegalStateException> { client.download { it.body.toByteArray(2) } }
        }
    }

    @Test fun optionalStreamChecksumsDoNotReadOrRejectAnUnrequestedBody() = realTest {
        var requests = 0
        streaming { request ->
            requests++
            assertNull(request.headers["x-algorithm"])
            assertNull(request.headers["x-amz-checksum-crc32"])
            assertEquals("123456789", request.body.read().decodeToString())
            respond("123456789", HttpStatusCode.OK)
        }.use { client ->
            assertEquals("123456789", client.transfer({ body = ByteStream.once(text("123456789")) }) { it.body.toByteArray(9).decodeToString() })
            var opens = 0
            client.transfer({ body = ByteStream.replay { opens++; text("123456789") } }) { it.body.toByteArray(9) }
            assertEquals(1, opens, "an optional checksum must not open a second reader")
            client.transfer({ body = ByteStream.fromBytes("123456789".encodeToByteArray()) }) { it.body.toByteArray(9) }
            assertEquals(3, requests)
        }
    }

    @Test fun requiredStreamChecksumsStillSelectTheDefaultAlgorithm() = realTest {
        var requests = 0
        streaming { request ->
            requests++
            assertEquals("CRC32", request.headers["x-algorithm"])
            assertEquals("123456789", request.body.read().decodeToString())
            assertEquals(if (requests == 1) "y/Q5Jg==" else "supplied", request.headers["x-amz-checksum-crc32"])
            respond("123456789", HttpStatusCode.OK)
        }.use { client ->
            var opens = 0
            client.requiredTransfer({ body = ByteStream.replay { opens++; text("123456789") } }) { it.body.toByteArray(9) }
            assertEquals(2, opens, "a required checksum keeps its checksum pass")
            assertFailsWith<IllegalArgumentException> { client.requiredTransfer({ body = ByteStream.once(text("123456789")) }) { } }
            assertEquals(1, requests, "a required one-shot checksum must fail before sending")
            client.requiredTransfer({ body = ByteStream.once(text("123456789")); checksum = "supplied" }) { it.body.toByteArray(9) }
            assertEquals(2, requests)
        }
    }

    @Test fun streamingChecksumsComputeReplayableBodiesAndVerifyWhileReading() = realTest {
        val vectors = linkedMapOf("CRC32" to "y/Q5Jg==", "CRC32C" to "4waSgw==", "CRC64NVME" to "rosUhgp5mIg=",
            "MD5" to "JfnnlDI7RTiF9RgfG2JNCw==", "SHA1" to "98O8HYCOBHMq32eZZczDTKeuNEE=", "SHA256" to "FeKw08M4keuw8e9gnsQZQgwg4yDOlMZfvIwzEkSOsiU=",
            "SHA512" to "2eZ2LdHI6vbWGzxhkvxAjU1tXxF20MKRabwk5xw/J0rSf81YEbMT1oH35V7ALXPUmclUVba1u1A6z1dPuo/+hQ==")
        val validated = mutableListOf<String>(); var sent: HttpRequestData? = null; var reply = "123456789"; var replyAlgorithm = "CRC32"
        streaming(validated = { operation, algorithm -> validated.add("$operation:$algorithm") }) { request ->
            sent = request; assertEquals("123456789", request.body.read().decodeToString())
            respond(reply, HttpStatusCode.OK, Headers.build { append("x-amz-checksum-" + replyAlgorithm.lowercase(), vectors.getValue(replyAlgorithm)) })
        }.use { client ->
            var opens = 0
            fun replay() = ByteStream.replay(9) { opens++; text("123456789") }
            assertEquals("123456789", client.transfer({ body = replay(); algorithm = TransferAlgorithm.CRC32 }) { it.body.toByteArray(9).decodeToString() })
            assertEquals(2, opens, "one checksum pass plus one send")
            assertEquals("CRC32", sent!!.headers["x-algorithm"]); assertEquals("ENABLED", sent!!.headers["x-validation"])
            assertEquals(vectors["CRC32"], sent!!.headers["x-amz-checksum-crc32"]); assertEquals(listOf("Transfer:CRC32"), validated)
            for ((name, value) in vectors) {
                replyAlgorithm = name
                client.transfer({ body = replay(); algorithm = TransferAlgorithm(name) }) { it.body.toByteArray(9) }
                assertEquals(value, sent!!.headers["x-amz-checksum-" + name.lowercase()]); assertEquals("Transfer:$name", validated.last())
            }
            opens = 0
            client.transfer({ body = ByteStream.fromBytes("123456789".encodeToByteArray()); algorithm = TransferAlgorithm.SHA256 }) { it.body.toByteArray(9) }
            assertEquals(vectors["SHA256"], sent!!.headers["x-amz-checksum-sha256"])
            replyAlgorithm = "CRC32"; val requests = sent
            assertFailsWith<IllegalArgumentException> { client.transfer({ body = ByteStream.once(text("123456789")); algorithm = TransferAlgorithm.CRC32 }) { } }
            assertTrue(requests === sent, "a one-shot body without a checksum is rejected before sending")
            client.transfer({ body = ByteStream.once(text("123456789")); algorithm = TransferAlgorithm.CRC32; checksum = "supplied" }) { it.body.toByteArray(9) }
            assertEquals("supplied", sent!!.headers["x-amz-checksum-crc32"])
            val count = validated.size
            client.transfer({ body = replay() }) { }
            assertEquals(count, validated.size, "an unread body is not reported as verified")
            reply = "12345678X"
            assertFailsWith<ChecksumMismatch> { client.transfer({ body = replay() }) { it.body.toByteArray(9) } }
            assertFailsWith<ChecksumMismatch> { client.transfer({ body = replay() }) { runCatching { it.body.toByteArray(9) }; "swallowed" } }
            assertEquals(count, validated.size)
        }
    }
}
