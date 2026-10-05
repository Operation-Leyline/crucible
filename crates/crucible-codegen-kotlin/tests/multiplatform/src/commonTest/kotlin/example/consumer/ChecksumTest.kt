package example.consumer

import example.weather.*
import example.weather.model.ChecksumAlgorithm
import example.weather.model.ChecksumValidation
import example.weather.model.ChecksumInput
import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.http.Headers
import io.ktor.http.HttpStatusCode
import io.ktor.http.content.OutgoingContent
import kotlin.test.*

class ChecksumTest {
    @Test fun generatedChecksums() = realTest {
        val vectors=linkedMapOf("CRC32" to "y/Q5Jg==","CRC32C" to "4waSgw==","CRC64NVME" to "rosUhgp5mIg=",
            "MD5" to "JfnnlDI7RTiF9RgfG2JNCw==","SHA1" to "98O8HYCOBHMq32eZZczDTKeuNEE=","SHA256" to "FeKw08M4keuw8e9gnsQZQgwg4yDOlMZfvIwzEkSOsiU=",
            "SHA512" to "2eZ2LdHI6vbWGzxhkvxAjU1tXxF20MKRabwk5xw/J0rSf81YEbMT1oH35V7ALXPUmclUVba1u1A6z1dPuo/+hQ==")
        var mode="normal";var observed:HttpRequestData?=null
        val http=HttpClient(MockEngine { request ->
            observed=request
            val body=(request.body as OutgoingContent.ByteArrayContent).bytes().decodeToString()
            val algorithm=request.headers["x-algorithm"]
            if(mode!="skip" && mode!="explicit") {
                val expected=if(body.isEmpty())"1B2M2Y8AsgTpgAmY7PhCfg==" else vectors[algorithm ?: "MD5"]
                assertEquals(expected,request.headers[if(algorithm==null)"content-md5" else "x-amz-checksum-"+algorithm.lowercase()])
            }
            val responseAlgorithm=algorithm ?: "CRC32"
            respond("123456789",HttpStatusCode.OK,Headers.build {
                if(mode!="unmodeled")append("x-amz-checksum-"+responseAlgorithm.lowercase(),if(mode=="bad")"AAAA" else vectors[responseAlgorithm]!!)
                append("x-amz-checksum-xxhash3","invalid ignored checksum")
                if(mode=="one")append("x-amz-checksum-sha256","AAAA")
            })
        })
        val validated=mutableListOf<String>();val config=ClientConfig(endpoint="https://example.test",httpClient=http,maxAttempts=1,checksumValidated={operation,algorithm->validated.add("$operation:$algorithm")})
        val bytes="123456789".encodeToByteArray()
        WeatherClient(config).use {client ->
            client.legacy { data=bytes };client.required { data=bytes };client.legacy { data=byteArrayOf() }
            val input=ChecksumInput { data=bytes };client.flexible(input)
            assertNull(input.algorithm);assertNull(input.validation)
            assertEquals("CRC32",observed!!.headers["x-algorithm"]);assertEquals("ENABLED",observed!!.headers["x-validation"])
            assertEquals(listOf("Flexible:CRC32"),validated)
            for(name in vectors.keys) {client.flexible { data=bytes;algorithm=ChecksumAlgorithm(name) };assertEquals("Flexible:$name",validated.last())}
            mode="bad";assertEquals("CRC32",assertFailsWith<ChecksumMismatch>{client.flexible(input)}.algorithm)
            mode="one";client.flexible(input)
            mode="unmodeled";val count=validated.size;client.flexible(input);assertEquals(count,validated.size)
            mode="explicit";client.flexible { data=bytes;checksum="supplied" };assertEquals("supplied",observed!!.headers["x-amz-checksum-crc32"])
            assertFailsWith<IllegalArgumentException> {client.flexible { data=bytes;algorithm=ChecksumAlgorithm.XXHASH64 }}
        }
        val optOut=config.copy(requestChecksumCalculation=ChecksumPolicy.WHEN_REQUIRED,responseChecksumValidation=ChecksumPolicy.WHEN_REQUIRED)
        WeatherClient(optOut).use { client ->
            mode="skip";client.flexible { data=bytes }
            assertNull(observed!!.headers["x-algorithm"]);assertNull(observed!!.headers["x-validation"]);assertNull(observed!!.headers["x-amz-checksum-crc32"])
            mode="normal";client.flexible { data=bytes;algorithm=ChecksumAlgorithm.SHA256;validation=ChecksumValidation.ENABLED }
        }
        WeatherClient(optOut.copy(middleware=listOf({it.body=bytes}))).use {client->client.legacy {data="changed".encodeToByteArray()} }
        http.close()
    }
}
