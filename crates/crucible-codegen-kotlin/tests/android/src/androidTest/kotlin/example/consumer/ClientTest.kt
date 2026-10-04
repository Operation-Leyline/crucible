package example.consumer

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import example.weather.Client
import example.weather.ClientConfig
import example.weather.Models
import example.weather.OptionalValue
import java.net.InetAddress
import java.net.ServerSocket
import java.time.Instant
import kotlin.concurrent.thread
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ClientTest {
    @Test fun weatherAndCancellation(): Unit = runBlocking {
        val endpoint = InstrumentationRegistry.getArguments().getString("endpoint")!!
        withTimeout(30000) {
            Client(ClientConfig(endpoint = endpoint, token = { "test-token" })).use { client ->
                val input = Models.CreateCityInput(name = OptionalValue.of("kotlin-android"),
                    location = OptionalValue.of(Models.Location.Unknown(Models.Unit)))
                val city = client.createCity(input)
                check(client.getCity(Models.GetCityInput(cityId = OptionalValue.of(city.city.id))).city.name == "kotlin-android")
                val photo = byteArrayOf(0, 1, -1, -128)
                val stored = client.putCityPhoto(Models.PutCityPhotoInput(cityId = OptionalValue.of(city.city.id), photo = OptionalValue.of(photo)))
                check(stored.photo.thumbnail.contentEquals(photo))
                check(client.getCityPhoto(Models.GetCityPhotoInput(cityId = OptionalValue.of(city.city.id))).photo.value!!.contentEquals(photo))
                try { client.createCity(input); error("Expected modeled conflict") } catch (_: Models.ConflictError) { }
                check(client.listCitiesPages(Models.ListCitiesInput(maxResults = OptionalValue.of(1))).toList()
                    .any { page -> page.cities.any { it.name == "kotlin-android" } })
                check(!input.clientToken.present)
            }
            val entered = CompletableDeferred<Unit>()
            val release = CompletableDeferred<String>()
            Client(ClientConfig(endpoint = endpoint, token = { entered.complete(Unit); release.await() })).use { client ->
                val pending = launch { client.ping() }
                entered.await()
                pending.cancelAndJoin()
                release.complete("test-token")
            }
        }
    }

    @Test fun exactNumbersAndNanosecondsOnAndroid() = runBlocking {
        ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { server ->
            val served = CompletableDeferred<Unit>()
            val worker = thread(isDaemon = true) {
                try {
                    server.accept().use { socket ->
                        socket.soTimeout = 5000
                        val reader = socket.getInputStream().bufferedReader()
                        while (!reader.readLine().isNullOrEmpty()) { }
                        val body = """{"city":{"id":"c","name":"x","population":9223372036854775807,"createdAt":-0.000000001}}""".toByteArray()
                        socket.getOutputStream().apply {
                            write("HTTP/1.1 200 OK\r\nContent-Length: ${body.size}\r\nConnection: close\r\n\r\n".toByteArray())
                            write(body); flush()
                        }
                    }
                    served.complete(Unit)
                } catch (error: Throwable) { served.completeExceptionally(error) }
            }
            withTimeout(10000) {
                Client(ClientConfig(endpoint = "http://127.0.0.1:${server.localPort}")).use { client ->
                    val city = client.getCity(Models.GetCityInput(cityId = OptionalValue.of("c"))).city
                    check(city.population == Long.MAX_VALUE)
                    check(city.createdAt.value == Instant.ofEpochSecond(-1, 999999999))
                }
                served.await()
            }
            worker.join(1000)
        }
    }
}
