package example.consumer

import example.weather.model.PutCityPhotoInput
import example.weather.model.Location
import example.weather.model.ListCitiesInput
import example.weather.model.GetCityPhotoInput
import example.weather.model.GetCityInput
import example.weather.model.CreateCityInput
import example.weather.model.ConflictError
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import example.weather.WeatherClient
import example.weather.ClientConfig
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
            WeatherClient(ClientConfig(endpoint = endpoint, token = { "test-token" })).use { client ->
                val input = CreateCityInput { this.name = "kotlin-android"; this.location = Location.Unknown(kotlin.Unit) }
                val city = client.createCity(input)
                check(client.getCity(GetCityInput { this.cityId = city.city.id }).city.name == "kotlin-android")
                val photo = byteArrayOf(0, 1, -1, -128)
                val stored = client.putCityPhoto(PutCityPhotoInput { this.cityId = city.city.id; this.photo = photo })
                check(stored.photo.thumbnail.contentEquals(photo))
                check(client.getCityPhoto(GetCityPhotoInput { this.cityId = city.city.id }).photo!!.contentEquals(photo))
                try { client.createCity(input); error("Expected modeled conflict") } catch (_: ConflictError) { }
                check(client.listCitiesPages(ListCitiesInput { this.maxResults = 1 }).toList()
                    .any { page -> page.cities.any { it.name == "kotlin-android" } })
                check(!input.hasClientToken())
            }
            val entered = CompletableDeferred<Unit>()
            val release = CompletableDeferred<String>()
            WeatherClient(ClientConfig(endpoint = endpoint, token = { entered.complete(Unit); release.await() })).use { client ->
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
                WeatherClient(ClientConfig(endpoint = "http://127.0.0.1:${server.localPort}")).use { client ->
                    val city = client.getCity(GetCityInput { this.cityId = "c" }).city
                    check(city.population == Long.MAX_VALUE)
                    check(city.createdAt == Instant.ofEpochSecond(-1, 999999999))
                }
                served.await()
            }
            worker.join(1000)
        }
    }
}
